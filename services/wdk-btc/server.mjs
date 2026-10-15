// PowerEdge treasury service on Tether WDK (@tetherto/wdk-wallet-btc), Mutinynet only.
//
// Read-only HTTP API on 127.0.0.1 (bearer token shared with the other PowerEdge services):
//   GET /health
//   GET /balance                      treasury BTC balance + receive address
//   GET /fees?count=N&vbytes=V        live fee rates + estimated cost of N RGB payouts
//   GET /check?count=N&speed=normal   can the treasury cover N payouts tonight?
//   GET /transfers?limit=N            recent treasury transfers
// Nothing here can send. The seed lives in a git-ignored 0600 file, never in logs.
import fs from 'node:fs'
import http from 'node:http'
import path from 'node:path'
import crypto from 'node:crypto'
import { fileURLToPath } from 'node:url'
import bip39 from 'bip39'
import WalletManagerBtc, { MempoolElectrumClient } from '@tetherto/wdk-wallet-btc'
import { payoutCost, coverage, RGB_PAYOUT_VBYTES, RGB_WITNESS_SAT } from './costs.mjs'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..')
const env = (name, fallback) => process.env[name] || fallback
const HOST = env('WDK_HOST', '127.0.0.1')
const PORT = Number(env('WDK_PORT', '8713'))
const SEED_FILE = env('WDK_SEED_FILE', path.join(ROOT, 'secrets', 'wdk-treasury.mnemonic'))
const TOKEN_FILE = env('WALLET_API_TOKEN_FILE', path.join(ROOT, 'secrets', 'wallet-api.token'))
const ELECTRUM = { host: env('WDK_ELECTRUM_HOST', 'electrum.mutinynet.com'), port: Number(env('WDK_ELECTRUM_PORT', '50002')), protocol: 'ssl' }

const log = (level, msg, extra = {}) => console.log(JSON.stringify({ t: new Date().toISOString(), level, msg, ...extra }))

if (!['127.0.0.1', '::1', 'localhost'].includes(HOST)) { log('error', 'refusing non-loopback bind', { HOST }); process.exit(2) }
if (env('POWEREDGE_NETWORK', 'mutinynet') !== 'mutinynet') { log('error', 'POWEREDGE_NETWORK must be mutinynet'); process.exit(2) }

function readOrCreateSeed () {
  if (!fs.existsSync(SEED_FILE)) {
    fs.mkdirSync(path.dirname(SEED_FILE), { recursive: true, mode: 0o700 })
    const fd = fs.openSync(SEED_FILE, 'wx', 0o600)
    fs.writeSync(fd, bip39.generateMnemonic() + '\n'); fs.fsyncSync(fd); fs.closeSync(fd)
    log('info', 'created treasury seed (git-ignored, 0600)', { file: SEED_FILE })
  }
  const seed = fs.readFileSync(SEED_FILE, 'utf8').trim()
  if (!bip39.validateMnemonic(seed)) throw new Error('treasury seed file is not a valid BIP-39 mnemonic')
  return seed
}

const token = fs.readFileSync(TOKEN_FILE, 'utf8').trim()
if (token.length < 32) { log('error', 'wallet API token too short'); process.exit(2) }
// Mutinynet uses testnet address/derivation parameters (tb1…, coin type 1).
const wallet = new WalletManagerBtc(readOrCreateSeed(), { network: 'testnet', client: new MempoolElectrumClient({ ...ELECTRUM, network: 'testnet' }) })
const account = await wallet.getAccount(0)
const address = await account.getAddress()

const authorized = (req) => {
  const supplied = Buffer.from(String(req.headers.authorization || '').replace(/^Bearer\s+/i, ''))
  const expected = Buffer.from(token)
  return supplied.length === expected.length && crypto.timingSafeEqual(supplied, expected)
}
const send = (res, status, body) => {
  res.writeHead(status, { 'content-type': 'application/json', 'cache-control': 'no-store' })
  res.end(JSON.stringify(body, (_, v) => (typeof v === 'bigint' ? Number(v) : v)))
}
const fail = (res, status, code, message) => send(res, status, { error: { code, message } })
const intParam = (url, name, def, max) => {
  const raw = url.searchParams.get(name)
  if (raw === null || raw === '') return def
  const n = Number(raw)
  if (!Number.isInteger(n) || n < 1 || n > max) throw new RangeError(`${name} must be 1..${max}`)
  return n
}

async function feeRates () {
  const rates = await wallet.getFeeRates()
  return { normal: Number(rates.normal), fast: Number(rates.fast) }
}

const routes = {
  '/health': async () => ({ ok: true, engine: 'Tether WDK wdk-wallet-btc', network: 'mutinynet' }),
  '/balance': async () => ({ network: 'mutinynet', engine: 'Tether WDK', address, balanceSat: Number(await account.getBalance()) }),
  '/fees': async (url) => {
    const count = intParam(url, 'count', 1, 1000)
    const vbytes = intParam(url, 'vbytes', RGB_PAYOUT_VBYTES, 100_000)
    return { network: 'mutinynet', engine: 'Tether WDK', ...payoutCost({ feeRates: await feeRates(), vbytes, count }), source: 'WDK fee rates × measured PowerEdge payout size' }
  },
  '/check': async (url) => {
    const count = intParam(url, 'count', 1, 1000)
    const speed = url.searchParams.get('speed') || 'normal'
    const cost = payoutCost({ feeRates: await feeRates(), count })
    const haveSat = Number(await account.getBalance())
    return { network: 'mutinynet', engine: 'Tether WDK', address, cost, coverage: coverage({ haveSat, cost, speed }) }
  },
  '/transfers': async (url) => {
    const limit = intParam(url, 'limit', 10, 100)
    const transfers = await account.getTransfers({ limit })
    return { address, transfers }
  },
}

http.createServer(async (req, res) => {
  const url = new URL(req.url, `http://${HOST}`)
  const handler = routes[url.pathname]
  if (req.method !== 'GET' || !handler) return fail(res, 404, 'not_found', 'no such endpoint')
  if (url.pathname !== '/health' && !authorized(req)) return fail(res, 401, 'unauthorized', 'missing or wrong token')
  try {
    send(res, 200, await handler(url))
  } catch (err) {
    const bad = err instanceof RangeError
    log(bad ? 'warn' : 'error', 'request failed', { path: url.pathname, error: String(err?.message || err).slice(0, 300) })
    fail(res, bad ? 400 : 502, bad ? 'invalid_request' : 'wdk_error', String(err?.message || err).slice(0, 300))
  }
}).listen(PORT, HOST, () => log('info', 'WDK treasury service ready', { host: HOST, port: PORT, address, witnessSat: RGB_WITNESS_SAT, payoutVbytes: RGB_PAYOUT_VBYTES }))

// PowerEdge treasury service on Tether WDK (@tetherto/wdk-wallet-btc), Mutinynet only.
//
// Read-only HTTP API on 127.0.0.1 (bearer token shared with the other PowerEdge services):
//   GET /health
//   GET /balance                      treasury BTC balance + receive address
//   GET /fees?count=N&vbytes=V        live fee rates + estimated cost of N RGB payouts
//   GET /check?count=N&speed=normal   can the treasury cover N payouts tonight?
//   GET /transfers?limit=N            recent treasury transfers
//   POST /door/attempts {label}       pay-to-enter: fresh treasury address for one PRICE-sat pass
//   GET /door/attempts/:id            pending | paid (+ pass) | expired; checks the chain while pending
//   GET /door/passes                  passes sold + revenue
// Nothing here can send. The seed lives in a git-ignored 0600 file, never in logs.
import fs from 'node:fs'
import http from 'node:http'
import path from 'node:path'
import crypto from 'node:crypto'
import { fileURLToPath } from 'node:url'
import bip39 from 'bip39'
import WalletManagerBtc, { MempoolElectrumClient } from '@tetherto/wdk-wallet-btc'
import { payoutCost, coverage, RGB_PAYOUT_VBYTES, RGB_WITNESS_SAT } from './costs.mjs'
import { Door, DOOR_PRICE_SAT } from './door.mjs'
import { withLink as linkWrapper } from './relink.mjs'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..')
const env = (name, fallback) => process.env[name] || fallback
const HOST = env('WDK_HOST', '127.0.0.1')
const PORT = Number(env('WDK_PORT', '8713'))
const SEED_FILE = env('WDK_SEED_FILE', path.join(ROOT, 'secrets', 'wdk-treasury.mnemonic'))
const TOKEN_FILE = env('WALLET_API_TOKEN_FILE', path.join(ROOT, 'secrets', 'wallet-api.token'))
const DOOR_DB = env('WDK_DOOR_DB', path.join(ROOT, 'datadirs', 'door', 'door.sqlite3'))
const DOOR_PRICE = Number(env('POWEREDGE_DOOR_PRICE_SAT', String(DOOR_PRICE_SAT)))
const ELECTRUM = { host: env('WDK_ELECTRUM_HOST', 'electrum.mutinynet.com'), port: Number(env('WDK_ELECTRUM_PORT', '50002')), protocol: 'ssl' }

const log = (level, msg, extra = {}) => console.log(JSON.stringify({ t: new Date().toISOString(), level, msg, ...extra }))

if (!['127.0.0.1', '::1', 'localhost'].includes(HOST)) { log('error', 'refusing non-loopback bind', { HOST }); process.exit(2) }
// Keys in a box: while the treasury's micro-VM owns the door data, never start a second treasury on the host.
if (fs.existsSync(path.join(ROOT, 'datadirs', 'wdk.IN_VM'))) { log('error', 'refusing: the WDK treasury lives in its micro-VM (datadirs/wdk.IN_VM)'); process.exit(3) }
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
// Electrum link: the default policy gives up after 2 retries 1 s apart and then every call fails
// with "Connection to server lost". Retry longer, and reconnect+retry once around every call.
const electrum = new MempoolElectrumClient({ ...ELECTRUM, network: 'testnet', maxRetry: 20, retryPeriod: 3000, pingPeriod: 45_000 })
const wallet = new WalletManagerBtc(readOrCreateSeed(), { network: 'testnet', client: electrum })
const withLink = linkWrapper({ reconnect: () => electrum.reconnect(), log })
setInterval(() => withLink(() => account.getBalance()).catch(e => log('warn', 'link watchdog', { error: String(e?.message || e).slice(0, 120) })), 60_000).unref()
const account = await wallet.getAccount(0)
const address = await account.getAddress()
const door = new Door({
  dbPath: DOOR_DB,
  priceSat: DOOR_PRICE,
  getAccount: async (i) => {
    const acct = await wallet.getAccount(i)
    return {
      getAddress: () => withLink(() => acct.getAddress()),
      getTransfers: (o) => withLink(() => acct.getTransfers(o)),
      getBalance: () => withLink(() => acct.getBalance()),
    }
  },
})

/** Treasury money = main account (0) + door accounts (1000+), both under the same WDK seed. */
async function treasury () {
  const mainSat = Number(await withLink(() => account.getBalance()))
  const { doorSat, doorAccounts } = await door.doorBalance()
  return { mainSat, doorSat, doorAccounts, totalSat: mainSat + doorSat }
}

const authorized = (req) => {
  const supplied = Buffer.from(String(req.headers.authorization || '').replace(/^Bearer\s+/i, ''))
  const expected = Buffer.from(token)
  return supplied.length === expected.length && crypto.timingSafeEqual(supplied, expected)
}
const readJson = (req) => new Promise((resolve, reject) => {
  let data = ''
  req.on('data', (c) => { data += c; if (data.length > 4096) { reject(new RangeError('body too large')); req.destroy() } })
  req.on('end', () => { try { resolve(data ? JSON.parse(data) : {}) } catch { reject(new RangeError('body must be JSON')) } })
  req.on('error', reject)
})
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

// Fee rates for THIS network. WDK's getFeeRates() asks mainnet mempool.space (hard-coded), which is
// the wrong chain for Mutinynet and may be unreachable at a venue; Mutinynet's own mempool answers
// in well under a second. Falls back to WDK, then to a floor, so a payout quote never hangs.
const FEE_URL = env('WDK_FEE_URL', 'https://mutinynet.com/api/v1/fees/recommended')
let lastRates = null
async function feeRates () {
  try {
    const r = await fetch(FEE_URL, { signal: AbortSignal.timeout(4000) })
    if (r.ok) {
      const f = await r.json()
      lastRates = { normal: Math.max(1, Math.round(f.hourFee ?? f.halfHourFee ?? 1)), fast: Math.max(1, Math.round(f.fastestFee ?? 2)), source: 'mutinynet mempool' }
      return lastRates
    }
  } catch (err) { log('warn', 'network fee source unavailable', { url: FEE_URL, error: String(err?.message || err).slice(0, 120) }) }
  try {
    const rates = await Promise.race([wallet.getFeeRates(), new Promise((_, rej) => setTimeout(() => rej(new Error('WDK fee timeout')), 4000))])
    lastRates = { normal: Number(rates.normal), fast: Number(rates.fast), source: 'WDK (mainnet mempool.space)' }
    return lastRates
  } catch (err) { log('warn', 'WDK fee rates unavailable', { error: String(err?.message || err).slice(0, 120) }) }
  return lastRates ? { ...lastRates, source: lastRates.source + ' (cached)' } : { normal: 1, fast: 2, source: 'floor (no fee source reachable)' }
}

const routes = {
  '/health': async () => ({ ok: true, engine: 'Tether WDK wdk-wallet-btc', network: 'mutinynet' }),
  // balanceSat stays the main account (older clients); totalSat includes the door's revenue.
  '/balance': async () => {
    const t = await treasury()
    return { network: 'mutinynet', engine: 'Tether WDK', address, balanceSat: t.mainSat, doorSat: t.doorSat, totalSat: t.totalSat, treasury: t, door: door.revenue() }
  },
  '/fees': async (url) => {
    const count = intParam(url, 'count', 1, 1000)
    const vbytes = intParam(url, 'vbytes', RGB_PAYOUT_VBYTES, 100_000)
    const rates = await feeRates()
    return { network: 'mutinynet', engine: 'Tether WDK', ...payoutCost({ feeRates: { normal: rates.normal, fast: rates.fast }, vbytes, count }), feeSource: rates.source, source: 'fee rate × measured PowerEdge payout size' }
  },
  '/check': async (url) => {
    const count = intParam(url, 'count', 1, 1000)
    const speed = url.searchParams.get('speed') || 'normal'
    const rates = await feeRates()
    const cost = payoutCost({ feeRates: { normal: rates.normal, fast: rates.fast }, count })
    const t = await treasury()
    return { network: 'mutinynet', engine: 'Tether WDK', address, cost, treasury: t, coverage: coverage({ haveSat: t.totalSat, cost, speed }) }
  },
  '/door/passes': async (url) => ({ ...door.revenue(), passes: door.passes(intParam(url, 'limit', 20, 500)) }),
  // Test hook (only with WDK_DEBUG=1): drop the Electrum socket as a flaky server would.
  ...(process.env.WDK_DEBUG === '1' ? { '/debug/drop-link': async () => { await electrum.close(); log('warn', 'debug: electrum link dropped on purpose'); return { dropped: true } } } : {}),
  '/transfers': async (url) => {
    const limit = intParam(url, 'limit', 10, 100)
    const transfers = await withLink(() => account.getTransfers({ limit }))
    return { address, transfers }
  },
}

http.createServer(async (req, res) => {
  const url = new URL(req.url, `http://${HOST}`)
  let handler = routes[url.pathname]
  const attempt = /^\/door\/attempts\/(\d{1,9})$/.exec(url.pathname)
  if (req.method === 'POST' && url.pathname === '/door/attempts') {
    handler = async () => door.start(await readJson(req))
  } else if (req.method === 'GET' && attempt) {
    handler = async () => (await door.check(Number(attempt[1]))) ?? fail(res, 404, 'not_found', 'no such door attempt')
  } else if (req.method !== 'GET' || !handler) {
    return fail(res, 404, 'not_found', 'no such endpoint')
  }
  if (url.pathname !== '/health' && !authorized(req)) return fail(res, 401, 'unauthorized', 'missing or wrong token')
  try {
    const body = await handler(url)
    if (body === undefined) return   // handler already answered (404)
    send(res, 200, body)
  } catch (err) {
    const bad = err instanceof RangeError
    log(bad ? 'warn' : 'error', 'request failed', { path: url.pathname, error: String(err?.message || err).slice(0, 300) })
    fail(res, bad ? 400 : 502, bad ? 'invalid_request' : 'wdk_error', String(err?.message || err).slice(0, 300))
  }
}).listen(PORT, HOST, () => log('info', 'WDK treasury service ready', { host: HOST, port: PORT, address, witnessSat: RGB_WITNESS_SAT, payoutVbytes: RGB_PAYOUT_VBYTES, doorPriceSat: DOOR_PRICE }))

// WDK spike (time-boxed, 2026-10-15): can WDK work on Mutinynet next to PowerEdge's rgb-lib wallets?
// Throwaway seeds generated in memory, temp datadirs under /tmp; no funds move. Run with Node 22
// (the @utexo/rgb-lib native addon does not compile on Node 24).
//   cd spikes/wdk && nvm use && node spike.mjs > ../../artifacts/wdk-spike/result.json
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import bip39 from 'bip39'

const MUTINYNET_ESPLORA = 'https://mutinynet.com/api'
const results = { node: process.version, startedAt: new Date().toISOString(), tests: [] }
const record = (name, fn) => results.tests.push({ name, ...fn })

async function step (name, fn) {
  const t0 = Date.now()
  try {
    const value = await fn()
    record(name, { ok: true, ms: Date.now() - t0, value })
  } catch (err) {
    record(name, { ok: false, ms: Date.now() - t0, error: String(err?.message || err).slice(0, 400) })
  }
}

const tmp = (label) => fs.mkdtempSync(path.join(os.tmpdir(), `wdk-spike-${label.replace(/[^a-z0-9]+/gi, '-')}-`))
const sbcInvoice = (() => {
  try { return JSON.parse(fs.readFileSync('/tmp/poweredge/flow-invoice.json', 'utf8')).invoice.invoice } catch { return null }
})()

// ---------------- A/B: WDK RGB ----------------
async function rgb (network, label) {
  const { default: WalletManagerRgb } = await import('@utexo/wdk-wallet-rgb')
  const seed = bip39.generateMnemonic()
  let account
  await step(`${label}: open account (network='${network}', indexer=Mutinynet Esplora)`, async () => {
    const manager = new WalletManagerRgb(seed, { network, dataDir: tmp(label), indexerUrl: MUTINYNET_ESPLORA })
    account = await manager.getAccount(0)
    return 'opened'
  })
  if (!account) return
  await step(`${label}: address`, async () => account.getAddress())
  await step(`${label}: register + sync against Mutinynet`, async () => {
    if (account.registerWallet) await account.registerWallet()
    if (account.syncWallet) await account.syncWallet()
    return { btcVanillaSettledSat: String(await account.getBalance()) }
  })
  await step(`${label}: list assets`, async () => account.listAssets())
  await step(`${label}: witness invoice (any asset)`, async () => {
    const r = await account.receiveAsset({ amount: 25, witness: true })
    return { invoice: r?.invoice, recipientId: r?.recipientId ?? r?.recipient_id }
  })
  if (sbcInvoice) {
    await step(`${label}: decode a PowerEdge (rgb-lib, Mutinynet sbc:) invoice`, async () => account.decodeRGBInvoice({ invoice: sbcInvoice }))
  }
}

// ---------------- C: WDK BTC ----------------
async function btc () {
  const { default: WalletManagerBtc, MempoolElectrumClient } = await import('@tetherto/wdk-wallet-btc')
  const seed = bip39.generateMnemonic()
  const client = new MempoolElectrumClient({ host: 'electrum.mutinynet.com', port: 50002, protocol: 'ssl', network: 'testnet' })
  let account
  await step("BTC: open account (network='testnet', Electrum electrum.mutinynet.com:50002 ssl)", async () => {
    const manager = new WalletManagerBtc(seed, { network: 'testnet', client })
    account = await manager.getAccount(0)
    return 'opened'
  })
  if (!account) return
  await step('BTC: address', async () => account.getAddress())
  await step('BTC: balance from Mutinynet', async () => String(await account.getBalance()))
  await step('BTC: fee rates from Mutinynet', async () => {
    const manager = new WalletManagerBtc(seed, { network: 'testnet', client })
    const rates = await manager.getFeeRates()
    return Object.fromEntries(Object.entries(rates).map(([k, v]) => [k, String(v)]))
  })
  await step('BTC: quote a 1,000 sat send (no broadcast)', async () => {
    const q = await account.quoteSendTransaction({ to: 'tb1pncw03gkwd65gydgq2pq499xq09nsdymhp6eara5cq9de8m9mjdzs3dd26s', value: 1000n })
    return { fee: String(q.fee) }
  })
  try { account.dispose?.() } catch {}
}

const which = process.argv[2] || 'all'
if (which === 'all' || which === 'rgb') {
  await rgb('signet', 'RGB/signet')
  await rgb('mutinynet', 'RGB/mutinynet')
}
if (which === 'all' || which === 'btc') await btc()
results.finishedAt = new Date().toISOString()
console.log(JSON.stringify(results, null, 2))
process.exit(0)

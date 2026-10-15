// Control: WDK RGB on standard Signet with its default indexer (no Mutinynet).
import fs from 'node:fs'; import os from 'node:os'; import path from 'node:path'; import bip39 from 'bip39'
const { default: WalletManagerRgb } = await import('@utexo/wdk-wallet-rgb')
const out = { node: process.version, tests: [] }
const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'wdk-spike-signet-default-'))
const t0 = Date.now()
try {
  const m = new WalletManagerRgb(bip39.generateMnemonic(), { network: 'signet', dataDir: dir })
  const a = await m.getAccount(0)
  await a.registerWallet?.(); await a.syncWallet?.()
  out.tests.push({ name: 'RGB/signet default indexer: register + sync', ok: true, ms: Date.now() - t0, balanceSat: String(await a.getBalance()) })
} catch (e) { out.tests.push({ name: 'RGB/signet default indexer: register + sync', ok: false, ms: Date.now() - t0, error: String(e?.message || e).slice(0, 300) }) }
console.log('@@RESULT@@' + JSON.stringify(out)); process.exit(0)

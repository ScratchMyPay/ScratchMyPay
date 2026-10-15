// Print the underlying rgb-lib error behind WDK's "Failed to establish online connection".
import fs from 'node:fs'; import os from 'node:os'; import path from 'node:path'; import bip39 from 'bip39'
const { default: WalletManagerRgb } = await import('@utexo/wdk-wallet-rgb')
const variants = [
  // (default UTEXO indexer and Mutinynet Electrum: InvalidElectrum, see artifacts)
  ['signet + Mutinynet Esplora', { network: 'signet', indexerUrl: 'https://mutinynet.com/api' }],

]
const out = []
for (const [name, cfg] of variants) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'wdk-spike-cause-'))
  try {
    const a = await new WalletManagerRgb(bip39.generateMnemonic(), { ...cfg, dataDir: dir }).getAccount(0)
    await a.syncWallet?.()
    out.push({ name, ok: true, balanceSat: String(await a.getBalance()) })
  } catch (e) {
    const chain = []; let c = e
    while (c && chain.length < 4) { chain.push(String(c?.message || c).slice(0, 300) + (c?.constructor?.name ? ` [${c.constructor.name}]` : '')); c = c.cause }
    out.push({ name, ok: false, errors: chain })
  }
}
console.log('@@RESULT@@' + JSON.stringify(out)); process.exit(0)

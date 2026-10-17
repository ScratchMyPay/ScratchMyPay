// Screenshots of "keys in a box" in the UI: the app's Keys strip and the editor palette's vm-status node.
//   cd scripts/e2e && node keys-shots.mjs [--prefix keys-]
import { chromium } from 'playwright-core'
import { parseArgs } from 'node:util'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const { values: a } = parseArgs({ options: {
  base: { type: 'string', default: 'http://127.0.0.1:1888' },
  prefix: { type: 'string', default: 'keys-' },
  chrome: { type: 'string', default: '/usr/bin/google-chrome' },
} })
const OUT = path.join(path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..'), 'artifacts', 'screenshots')
const browser = await chromium.launch({ executablePath: a.chrome, headless: true })
const page = await browser.newPage({ viewport: { width: 1280, height: 860 } })
page.on('dialog', d => d.dismiss())

await page.goto(a.base + '/poweredge-app', { waitUntil: 'networkidle' })
if (page.url().endsWith('/login')) {
  await page.click('#start'); await page.waitForSelector('#pay:not(.hidden)'); await page.click('#demo-pay')
  await page.waitForFunction(() => document.querySelector('#status')?.classList.contains('ok'), null, { timeout: 120000 })
  await page.waitForURL('**/poweredge-app', { timeout: 15000 })
}
await page.waitForSelector('#keys:not([hidden]) .keybox', { timeout: 30000 })
await page.waitForTimeout(500)
const strip = await page.$$eval('#keys .keybox', bs => bs.map(b => `${b.className.replace('keybox ', '')}: ${b.innerText.replace(/\s+/g, ' ')}`))
console.log('STRIP', JSON.stringify(strip))
const data = await page.evaluate(async () => {
  const token = localStorage.getItem('pe.pass') || sessionStorage.getItem('pe.pass') || ''
  const r = await fetch('/poweredge-app/api/keys', { headers: token ? { Authorization: 'Bearer ' + token } : {} })
  return r.ok ? r.json() : { status: r.status }
})
console.log('API', JSON.stringify(data).slice(0, 1500))
await page.screenshot({ path: `${OUT}/${a.prefix}app-strip.png`, clip: { x: 0, y: 0, width: 1280, height: 260 } })
await page.screenshot({ path: `${OUT}/${a.prefix}app.png` })

// editor: the palette category "Keys in a box"
await page.goto(a.base + '/', { waitUntil: 'networkidle' }).catch(() => {})
await page.waitForTimeout(4000)
const paletteHit = await page.$('[data-palette-type="vm-status"], .red-ui-palette-node[data-palette-type="vm-status"]')
console.log('EDITOR', page.url(), paletteHit ? 'vm-status in palette' : 'vm-status NOT found in palette')
await page.screenshot({ path: `${OUT}/${a.prefix}editor.png` })
await browser.close()

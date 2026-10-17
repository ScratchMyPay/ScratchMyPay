// Screenshot of Settings -> "n2link" (outbound policy) as a door-pass user, and whether saving is refused.
//   cd scripts/e2e && node egress-shot.mjs
import { chromium } from 'playwright-core'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const BASE = process.env.BASE || 'http://127.0.0.1:1888'
const browser = await chromium.launch({ executablePath: '/usr/bin/google-chrome', headless: true })
const page = await browser.newPage({ viewport: { width: 1400, height: 860 } })
page.on('dialog', d => d.dismiss())
await page.goto(BASE + '/poweredge-app', { waitUntil: 'networkidle' })
if (page.url().endsWith('/login')) {
  await page.click('#start'); await page.waitForSelector('#pay:not(.hidden)'); await page.click('#demo-pay')
  await page.waitForFunction(() => document.querySelector('#status')?.classList.contains('ok'), null, { timeout: 120000 })
  await page.waitForURL('**/poweredge-app', { timeout: 15000 })
}
await page.goto(BASE + '/', { waitUntil: 'networkidle' })
await page.waitForFunction(() => window.RED?.userSettings, null, { timeout: 30000 })
const read = await page.evaluate(async () => {
  const r = await $.ajax({ url: RED.settings.apiRootUrl + 'runtime/config/egress', dataType: 'json' }).then(d => ({ ok: true, d }), x => ({ ok: false, status: x.status }))
  return r.ok ? { status: 200, mode: r.d?.config?.mode ?? r.d?.mode, keys: Object.keys(r.d || {}) } : r
})
console.log('READ', JSON.stringify(read))
const write = await page.evaluate(async () => {
  try { await $.ajax({ url: RED.settings.apiRootUrl + 'runtime/config/egress/validate', method: 'POST', contentType: 'application/json', data: '{}' }); return 'allowed' }
  catch (x) { return `refused ${x.status}` }
})
console.log('WRITE', write)
await page.evaluate(() => RED.userSettings.show('n2link-config'))
await page.waitForTimeout(2500)
await page.screenshot({ path: path.join(ROOT, 'artifacts/screenshots/egress-settings.png') })
await browser.close()

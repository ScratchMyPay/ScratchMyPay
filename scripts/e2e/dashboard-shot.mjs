// Screenshots of the show-night dashboard (/poweredge-app/dashboard) on desktop and phone widths.
//   cd scripts/e2e && node dashboard-shot.mjs
import { chromium } from 'playwright-core'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const BASE = process.env.BASE || 'http://127.0.0.1:1888'
const OUT = path.join(ROOT, 'artifacts', 'screenshots')
const browser = await chromium.launch({ executablePath: '/usr/bin/google-chrome', headless: true })
const ctx = await browser.newContext({ viewport: { width: 1360, height: 900 } })
const page = await ctx.newPage()
page.on('dialog', d => d.dismiss())
await page.goto(BASE + '/poweredge-app', { waitUntil: 'networkidle' })
if (page.url().endsWith('/login')) {
  await page.click('#start'); await page.waitForSelector('#pay:not(.hidden)'); await page.click('#demo-pay')
  await page.waitForFunction(() => document.querySelector('#status')?.classList.contains('ok'), null, { timeout: 120000 })
  await page.waitForURL('**/poweredge-app', { timeout: 15000 })
}
await page.click('a[href="/poweredge-app/dashboard"]')
await page.waitForURL('**/poweredge-app/dashboard')
await page.waitForSelector('.card', { timeout: 30000 })
await page.waitForTimeout(3500)
const cards = await page.$$eval('.card', cs => cs.map(c => c.querySelector('h3')?.textContent + ': ' + (c.querySelector('.value')?.innerText || c.querySelector('svg') ? (c.querySelector('.value')?.innerText || 'chart') : (c.querySelector('table') ? 'table' : c.innerText.slice(0, 40)))))
console.log('CARDS', JSON.stringify(cards))
console.log('SECTIONS', JSON.stringify(await page.$$eval('section h2', hs => hs.map(h => h.textContent))))
await page.screenshot({ path: `${OUT}/dash-page.png`, fullPage: true })
await page.setViewportSize({ width: 390, height: 844 })
await page.waitForTimeout(800)
await page.screenshot({ path: `${OUT}/dash-page-phone.png` })
await browser.close()

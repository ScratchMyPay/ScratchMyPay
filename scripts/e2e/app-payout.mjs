// End-to-end payout through the guided app, in a real (headless) browser, with screenshots.
// crew request (crew tab) -> describe -> review -> approve -> track until settled.
//
//   cd scripts/e2e && npm install --no-save playwright-core@1.47.0
//   node app-payout.mjs --amount 1 --ticker USDT --prefix usdt-
//
// Buys a door pass with the organizer wallet's demo button if needed (test sats).
import { chromium } from 'playwright-core'
import { parseArgs } from 'node:util'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const { values: a } = parseArgs({ options: {
  base: { type: 'string', default: 'http://127.0.0.1:1888' },
  amount: { type: 'string', default: '1' },
  ticker: { type: 'string', default: 'USDT' },
  who: { type: 'string', default: 'sound engineer' },
  why: { type: 'string', default: 'the Saturday load-in' },
  prefix: { type: 'string', default: '' },
  invoice: { type: 'string', default: '' },   // pay this invoice instead of requesting one in the crew tab
  chrome: { type: 'string', default: '/usr/bin/google-chrome' },
} })
const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const OUT = path.join(ROOT, 'artifacts', 'screenshots')
const t0 = Date.now(); const log = (m) => console.log(`[${((Date.now() - t0) / 1000).toFixed(0)}s] ${m}`)
const browser = await chromium.launch({ executablePath: a.chrome, headless: true })
const page = await browser.newPage({ viewport: { width: 1280, height: 860 } })
page.on('dialog', d => d.dismiss())
const shot = (n) => page.screenshot({ path: `${OUT}/${a.prefix}${n}.png` })

await page.goto(a.base + '/poweredge-app', { waitUntil: 'networkidle' })
if (page.url().endsWith('/login')) {
  await page.click('#start'); await page.waitForSelector('#pay:not(.hidden)'); await page.click('#demo-pay')
  await page.waitForFunction(() => document.querySelector('#status')?.classList.contains('ok'), null, { timeout: 120000 })
  await page.waitForURL('**/poweredge-app', { timeout: 15000 })
  log('bought a door pass')
}
await page.waitForFunction(() => document.querySelector('#ai-badge')?.textContent?.includes('QVAC'), null, { timeout: 60000 })

// crew: payment request (or a given invoice)
if (!a.invoice) {
await page.click('#tab-crew'); await page.fill('#crew-name', a.who); await page.fill('#crew-amount', a.amount)
await page.click('#crew-go')
await page.waitForFunction(() => document.querySelector('#crew-invoice')?.value?.startsWith('rgb:'), null, { timeout: 120000 })
const invoice = await page.inputValue('#crew-invoice')
log('crew invoice ' + invoice.slice(0, 40) + '… proxy: ' + invoice.includes('endpoints='))
await page.waitForTimeout(600); await shot('app-crew-request')
}

// organizer: describe -> review
await page.click('#tab-org'); await page.fill('#prompt', `Pay the ${a.who} ${a.amount} ${a.ticker} for ${a.why}`)
if (a.invoice) await page.fill('#invoice', a.invoice); else await page.click('#paste-latest')
await page.waitForTimeout(400); await shot('app-describe')
await page.click('#go-draft')
await page.waitForSelector('#go-setup:not([disabled])', { timeout: 180000 }); await page.waitForTimeout(600)
await shot('app-review'); log('review: ' + (await page.textContent('#r-headline')).trim())

// approve -> send once
await page.click('#go-setup')
await page.waitForSelector('#actor', { timeout: 180000 }); await page.waitForTimeout(800)
await page.fill('#actor', 'Gus (organizer)'); await page.check('#a-check'); await page.waitForTimeout(300)
await shot('app-approve'); log('approve: ' + (await page.textContent('#a-headline')).trim())
await page.click('#go-approve')

// track until settled (via the proxy: settles after the crew wallet's ACK)
await page.waitForFunction(() => !document.querySelector('#receipt')?.classList.contains('hidden'), null, { timeout: 900000 })
await page.waitForTimeout(800); await shot('app-track-settled')
log('settled: ' + (await page.innerText('#receipt')).replace(/\s+/g, ' ').slice(0, 300))
await browser.close()

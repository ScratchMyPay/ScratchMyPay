// Set host/port of an `http in` node (n2link extension fields, not shown in the stock Node-RED form),
// through the editor's own API, then Deploy. Logs in as the break-glass operator (no door pass).
//   cd scripts/e2e && node set-http-port.mjs --url /pay-drummer --port 1896 [--host 127.0.0.1]
import { chromium } from 'playwright-core'
import { readFileSync } from 'node:fs'
import { parseArgs } from 'node:util'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const { values: a } = parseArgs({ options: {
  url: { type: 'string' }, port: { type: 'string' }, host: { type: 'string', default: '127.0.0.1' },
  base: { type: 'string', default: 'http://127.0.0.1:1888' },
} })
const password = readFileSync(path.join(ROOT, 'secrets', 'operator.password'), 'utf8').trim()
const login = await fetch(a.base + '/auth/token', {
  method: 'POST', headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
  body: new URLSearchParams({ client_id: 'node-red-editor', grant_type: 'password', scope: '*', username: 'operator', password }),
})
const tokens = await login.json()
if (!tokens.access_token) throw new Error(`operator login failed (HTTP ${login.status})`)

const browser = await chromium.launch({ executablePath: '/usr/bin/google-chrome', headless: true })
const page = await browser.newPage({ viewport: { width: 1400, height: 900 } })
await page.goto(a.base + '/poweredge-app/login')
await page.evaluate(t => localStorage.setItem('auth-tokens', JSON.stringify(t)), tokens)
await page.goto(a.base + '/', { waitUntil: 'networkidle' })
await page.waitForFunction(() => window.RED?.nodes && RED.workspaces?.count?.() > 0, null, { timeout: 30000 })
const result = await page.evaluate(([url, port, host]) => {
  const hits = []
  RED.nodes.eachNode(n => { if (n.type === 'http in' && n.url === url) hits.push(n) })
  if (hits.length !== 1) return { error: `found ${hits.length} http in nodes for ${url}` }
  const n = hits[0], before = { host: n.host, port: n.port }
  n.host = host; n.port = Number(port); n.changed = true; n.dirty = true
  RED.nodes.dirty(true); RED.view.redraw()
  return { id: n.id, before, after: { host: n.host, port: n.port } }
}, [a.url, a.port, a.host])
console.log('NODE', JSON.stringify(result))
if (!result.error) {
  await page.click('#red-ui-header-button-deploy')
  await page.waitForTimeout(2500)
  console.log('deploy:', (await page.$eval('.red-ui-notification', e => e.innerText).catch(() => '')).replace(/\s+/g, ' ').slice(0, 100))
}
await browser.close()

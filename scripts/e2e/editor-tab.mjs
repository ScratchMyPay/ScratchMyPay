// Deploy a Flow Library seed as an editor tab, through the editor's own import, then Deploy.
// Fills wdk-wallet-ref references with the editor's (single) treasury config node.
// Adds a tab only; the managed payout tab is not touched.
//   cd scripts/e2e && node editor-tab.mjs --seed library/seed/50-show-night-dashboard.json --label "Show-night dashboard" [--replace]
import { chromium } from 'playwright-core'
import { readFileSync } from 'node:fs'
import { parseArgs } from 'node:util'
import { fileURLToPath } from 'node:url'
import path from 'node:path'
import crypto from 'node:crypto'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const { values: a } = parseArgs({ options: {
  seed: { type: 'string' }, label: { type: 'string' }, replace: { type: 'boolean', default: false },
  base: { type: 'string', default: 'http://127.0.0.1:1888' }, shot: { type: 'string', default: '' },
} })
const seed = JSON.parse(readFileSync(path.resolve(ROOT, a.seed), 'utf8'))
const label = a.label || seed.name
const id = () => crypto.randomBytes(8).toString('hex')
const WDK_REF_TYPES = new Set(['door-stats', 'wdk-fee-quote', 'wdk-balance', 'wdk-transfers'])

const tab = { id: id(), type: 'tab', label, info: seed.description, disabled: false }
const ids = Object.fromEntries(seed.draft.nodes.map(n => [n.ref, id()]))
const outputsOf = n => ['debug', 'ui-stat', 'ui-chart', 'ui-table'].includes(n.type) ? 0 : n.type === 'switch' ? (n.config.outputs || 1) : 1
const nodes = seed.draft.nodes.map(n => {
  const wires = Array.from({ length: outputsOf(n) }, (_, port) =>
    seed.draft.wires.filter(w => w.from === n.ref && (w.output || 0) === port).map(w => ids[w.to]))
  return { id: ids[n.ref], type: n.type, z: tab.id, name: n.name || '', x: n.x, y: n.y, ...n.config, wires }
})

const browser = await chromium.launch({ executablePath: '/usr/bin/google-chrome', headless: true })
const page = await browser.newPage({ viewport: { width: 1400, height: 900 } })
page.on('dialog', d => d.dismiss())
await page.goto(a.base + '/poweredge-app', { waitUntil: 'networkidle' })
if (page.url().endsWith('/login')) {
  await page.click('#start'); await page.waitForSelector('#pay:not(.hidden)'); await page.click('#demo-pay')
  await page.waitForFunction(() => document.querySelector('#status')?.classList.contains('ok'), null, { timeout: 120000 })
  await page.waitForURL('**/poweredge-app', { timeout: 15000 })
}
await page.goto(a.base + '/', { waitUntil: 'networkidle' })
await page.waitForFunction(() => window.RED?.nodes && RED.workspaces?.count?.() > 0, null, { timeout: 30000 })
const found = await page.evaluate(l => {
  let tabId = null, wdk = [];
  RED.nodes.eachWorkspace(w => { if (w.label === l) tabId = w.id })
  RED.nodes.eachConfig(c => { if (c.type === 'wdk-wallet-ref') wdk.push(c.id) })
  return { tabId, wdk }
}, label)
if (found.wdk.length !== 1 && nodes.some(n => WDK_REF_TYPES.has(n.type))) throw new Error(`expected one wdk-wallet-ref, found ${found.wdk.length}`)
for (const n of nodes) if (WDK_REF_TYPES.has(n.type)) n.wallet = found.wdk[0]

if (found.tabId && !a.replace) {
  console.log('tab already exists:', found.tabId)
  await page.evaluate(t => RED.workspaces.show(t), found.tabId)
} else {
  if (found.tabId) await page.evaluate(t => RED.workspaces.delete(RED.nodes.workspace(t)), found.tabId)
  await page.evaluate(([t, ns]) => {
    RED.view.importNodes([t, ...ns], { addFlow: false, generateIds: false, applyNodeDefaults: true, notify: false })
    RED.workspaces.show(t.id)
  }, [tab, nodes])
  await page.click('#red-ui-header-button-deploy')
  await page.waitForTimeout(2000)
  const note = await page.$eval('.red-ui-notification', e => e.innerText).catch(() => '')
  console.log('deploy:', note.replace(/\s+/g, ' ').slice(0, 120) || '(no notification)')
}
await page.waitForTimeout(9000)
const statuses = await page.$$eval('.red-ui-flow-node-status-label', ls => ls.map(l => l.textContent.trim()).filter(Boolean))
console.log('STATUS', JSON.stringify(statuses).slice(0, 1500))
if (a.shot) await page.screenshot({ path: path.join(ROOT, 'artifacts/screenshots', a.shot) })
await browser.close()

// Add a "Keys in a box" tab to the flow editor, built from the Flow Library seed
// library/seed/40-keys-in-a-box.json, through the editor's own import, then Deploy. Adds a tab only;
// the managed payout tab is not touched. Skips if the tab already exists. Takes a screenshot.
//   cd scripts/e2e && node editor-keys-tab.mjs
import { chromium } from 'playwright-core'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import path from 'node:path'
import crypto from 'node:crypto'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const BASE = process.env.BASE || 'http://127.0.0.1:1888'
const seed = JSON.parse(readFileSync(path.join(ROOT, 'library/seed/40-keys-in-a-box.json'), 'utf8'))
const id = () => crypto.randomBytes(8).toString('hex')

// Draft (refs + config) -> Node-RED nodes, the same shape the Copilot imports.
const tab = { id: id(), type: 'tab', label: 'Keys in a box', info: seed.description, disabled: false }
const ids = Object.fromEntries(seed.draft.nodes.map(n => [n.ref, id()]))
const outputsOf = n => n.type === 'debug' ? 0 : n.type === 'switch' ? (n.config.outputs || 1) : 1
const nodes = seed.draft.nodes.map(n => {
  const wires = Array.from({ length: outputsOf(n) }, (_, port) =>
    seed.draft.wires.filter(w => w.from === n.ref && (w.output || 0) === port).map(w => ids[w.to]))
  return { id: ids[n.ref], type: n.type, z: tab.id, name: n.name || '', x: n.x, y: n.y, ...n.config, wires }
})

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
await page.waitForFunction(() => window.RED?.nodes && RED.workspaces?.count?.() > 0, null, { timeout: 30000 })
const existing = await page.evaluate(() => { let t = null; RED.nodes.eachWorkspace(w => { if (w.label === 'Keys in a box') t = w.id }); return t })
if (existing && !process.argv.includes('--replace')) {
  console.log('tab already exists:', existing)
  await page.evaluate(t => RED.workspaces.show(t), existing)
} else {
  if (existing) {   // --replace: remove the old tab and its nodes first
    await page.evaluate(t => { const ws = RED.nodes.workspace(t); RED.workspaces.delete(ws) }, existing)
  }
  await page.evaluate(([t, ns]) => {
    RED.view.importNodes([t, ...ns], { addFlow: false, generateIds: false, applyNodeDefaults: true, notify: false })
    RED.workspaces.show(t.id)
  }, [tab, nodes])
  await page.click('#red-ui-header-button-deploy')
  await page.waitForTimeout(1500)
  const confirm = await page.$('.red-ui-notification button.primary, #red-ui-header-button-deploy-confirm')
  if (confirm) await confirm.click().catch(() => {})
  console.log('tab added and deployed')
}
await page.waitForTimeout(9000)   // inject fires 2 s after deploy; let the status dots settle
const statuses = await page.$$eval('.red-ui-flow-node-status-label', ls => ls.map(l => l.textContent.trim()).filter(Boolean))
console.log('STATUS', JSON.stringify(statuses))
await page.screenshot({ path: path.join(ROOT, 'artifacts/screenshots/keys-editor-tab.png') })
await browser.close()

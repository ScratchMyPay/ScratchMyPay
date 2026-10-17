// Ask Flow Copilot (local QVAC) for a draft exactly like the editor sidebar does, and print the summary
// and the endpoints n2link assigned. Draft only: nothing is applied or deployed.
// Logs in as the break-glass operator (no door pass).
//   cd scripts/e2e && node copilot-draft.mjs --tab "Flow 1" --prompt "Make a web page at /pay-bassist ..."
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs'
import { parseArgs } from 'node:util'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const { values: a } = parseArgs({ options: {
  tab: { type: 'string', default: 'Flow 1' },
  prompt: { type: 'string', default: 'Make a web page at /pay-bassist that creates an RGB payment request and shows it as a QR code.' },
  out: { type: 'string', default: '' },
  base: { type: 'string', default: 'http://127.0.0.1:1888' },
} })
const password = readFileSync(path.join(ROOT, 'secrets', 'operator.password'), 'utf8').trim()
const login = await fetch(a.base + '/auth/token', {
  method: 'POST', headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
  body: new URLSearchParams({ client_id: 'node-red-editor', grant_type: 'password', scope: '*', username: 'operator', password }),
})
const token = (await login.json()).access_token
if (!token) throw new Error(`operator login failed (HTTP ${login.status})`)
const auth = { Authorization: 'Bearer ' + token }
const flowsRes = await (await fetch(a.base + '/flows', { headers: { ...auth, 'Node-RED-API-Version': 'v2' } })).json()
const flows = Array.isArray(flowsRes) ? flowsRes : flowsRes.flows
const tab = flows.find(n => n.type === 'tab' && n.label === a.tab)
const provider = flows.find(n => n.type === 'ai-provider')
if (!tab || !provider) throw new Error(`need tab '${a.tab}' and an ai-provider (found tab=${!!tab}, provider=${!!provider})`)
const t0 = Date.now()
const r = await fetch(a.base + '/assistant/draft', {
  method: 'POST', headers: { ...auth, 'Content-Type': 'application/json' },
  body: JSON.stringify({ providerId: provider.id, model: '', prompt: a.prompt, workspaceId: tab.id, workspaceLabel: tab.label, flows }),
})
const body = await r.json()
console.log('HTTP', r.status, `${Date.now() - t0} ms`)
console.log('SUMMARY', body.summary || JSON.stringify(body.error))
console.log('NODES', (body.nodes || []).map(n => `${n.type}${n.type === 'http in' ? ` ${n.host}:${n.port}${n.url}` : ''}`).join(' -> '))
console.log('ENDPOINTS', JSON.stringify(body.endpoints, null, 1))
if (a.out) {
  const out = path.resolve(ROOT, a.out); mkdirSync(path.dirname(out), { recursive: true })
  writeFileSync(out, JSON.stringify({ capturedAt: new Date().toISOString(), tab: a.tab, prompt: a.prompt, status: r.status, body }, null, 1) + '\n')
}

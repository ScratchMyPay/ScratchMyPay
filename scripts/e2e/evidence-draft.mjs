// Capture one real QVAC draft from the guided app for the submission evidence: the prompt, QVAC's
// structured intent, its explanation, model and latency, the pinned template (id/version/digest) and
// the compiled flow (digest, node count). Nothing is approved or sent.
// Logs in as the break-glass operator (secrets/operator.password) so no door pass is bought.
//   cd scripts/e2e && node evidence-draft.mjs [--prompt "..."] [--out artifacts/evidence/qvac-draft.json]
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs'
import { parseArgs } from 'node:util'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const { values: a } = parseArgs({ options: {
  prompt: { type: 'string', default: "Pay the sound engineer 1 USDT for Sunday's demo" },
  out: { type: 'string', default: 'artifacts/evidence/qvac-draft.json' },
  base: { type: 'string', default: 'http://127.0.0.1:1888' },
} })
const password = readFileSync(path.join(ROOT, 'secrets', 'operator.password'), 'utf8').trim()
const login = await fetch(a.base + '/auth/token', {
  method: 'POST', headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
  body: new URLSearchParams({ client_id: 'node-red-editor', grant_type: 'password', scope: '*', username: 'operator', password }),
})
const token = (await login.json()).access_token
if (!token) throw new Error(`operator login failed (HTTP ${login.status})`)
const r = await fetch(a.base + '/poweredge-app/api/draft', {
  method: 'POST', headers: { 'Content-Type': 'application/json', Authorization: 'Bearer ' + token }, body: JSON.stringify({ prompt: a.prompt }),
})
const body = await r.json()
const out = path.resolve(ROOT, a.out)
mkdirSync(path.dirname(out), { recursive: true })
writeFileSync(out, JSON.stringify({ capturedAt: new Date().toISOString(), prompt: a.prompt, note: 'draft only: nothing approved or sent', status: r.status, body }, null, 1) + '\n')
console.log('DRAFT', r.status, body.ok, body.template?.id, body.template?.digest?.slice(0, 12), 'flow nodes', body.flow?.nodes, 'model', body.ai?.modelLabel, `${body.ai?.latencyMs} ms`)

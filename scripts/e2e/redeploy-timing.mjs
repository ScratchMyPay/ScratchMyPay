// How long a full Deploy takes (stop every flow, start them again), via the editor's own /flows API.
// Redeploys the flows unchanged; prints the time until the payout endpoints listen again.
//   cd scripts/e2e && node redeploy-timing.mjs [--runs 3] [--gap 13]
import { readFileSync } from 'node:fs'
import { parseArgs } from 'node:util'
import { fileURLToPath } from 'node:url'
import net from 'node:net'
import path from 'node:path'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const { values: a } = parseArgs({ options: {
  runs: { type: 'string', default: '3' }, gap: { type: 'string', default: '13' },
  base: { type: 'string', default: 'http://127.0.0.1:1888' },
} })
const password = readFileSync(path.join(ROOT, 'secrets', 'operator.password'), 'utf8').trim()
const login = await fetch(a.base + '/auth/token', {
  method: 'POST', headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
  body: new URLSearchParams({ client_id: 'node-red-editor', grant_type: 'password', scope: '*', username: 'operator', password }),
})
const token = (await login.json()).access_token
const auth = { Authorization: 'Bearer ' + token, 'Node-RED-API-Version': 'v2' }
const listening = port => new Promise(res => {
  const s = net.connect({ host: '127.0.0.1', port }, () => { s.destroy(); res(true) })
  s.on('error', () => res(false))
})
const sleep = ms => new Promise(r => setTimeout(r, ms))
for (let i = 1; i <= Number(a.runs); i++) {
  const current = await (await fetch(a.base + '/flows', { headers: auth })).json()
  const t0 = Date.now()
  const r = await fetch(a.base + '/flows', {
    method: 'POST', headers: { ...auth, 'Content-Type': 'application/json', 'Node-RED-Deployment-Type': 'full' },
    body: JSON.stringify({ flows: current.flows, rev: current.rev }),
  })
  const answered = Date.now() - t0
  let up = -1
  for (let w = 0; w < 600; w++) {
    const ports = await Promise.all([1891, 1892, 1893, 1894].map(listening))
    if (ports.every(Boolean)) { up = Date.now() - t0; break }
    await sleep(200)
  }
  console.log(`run ${i}: POST /flows HTTP ${r.status} after ${answered} ms; payout endpoints listening after ${up} ms`)
  await sleep(Number(a.gap) * 1000)
}

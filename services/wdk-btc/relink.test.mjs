// A dropped Electrum link must heal on the next call: reconnect once, then retry the call.
import test from 'node:test'
import assert from 'node:assert/strict'
import { withLink, LINK_DOWN } from './relink.mjs'

test('reconnects once and retries when the link is down', async () => {
  let reconnects = 0
  const link = withLink({ reconnect: async () => { reconnects++ } , log: () => {} })
  let calls = 0
  const out = await link(async () => { calls++; if (calls === 1) throw new Error('Connection to server lost, please retry'); return 42 })
  assert.equal(out, 42); assert.equal(calls, 2); assert.equal(reconnects, 1)
})

test('other errors pass through untouched, no reconnect', async () => {
  let reconnects = 0
  const link = withLink({ reconnect: async () => { reconnects++ }, log: () => {} })
  await assert.rejects(() => link(async () => { throw new RangeError('bad amount') }), RangeError)
  assert.equal(reconnects, 0)
})

test('concurrent calls share one reconnect', async () => {
  let reconnects = 0
  const link = withLink({ reconnect: () => new Promise(r => setTimeout(() => { reconnects++; r() }, 30)), log: () => {} })
  const flaky = () => { let n = 0; return async () => { if (n++ === 0) throw new Error('socket hang up'); return 'ok' } }
  const results = await Promise.all([link(flaky()), link(flaky()), link(flaky())])
  assert.deepEqual(results, ['ok', 'ok', 'ok']); assert.equal(reconnects, 1)
})

test('link-down matcher', () => {
  for (const m of ['Connection to server lost, please retry', 'socket hang up', 'read ECONNRESET', 'Request timed out']) assert.ok(LINK_DOWN.test(m), m)
  assert.ok(!LINK_DOWN.test('insufficient funds'))
})

test("in-flight requests rejected with 'close connect' when the socket closes are link errors", async () => {
  assert.ok(LINK_DOWN.test('close connect'))
  let reconnects = 0
  const link = withLink({ reconnect: async () => { reconnects++ }, log: () => {} })
  let calls = 0
  const out = await link(async () => { calls++; if (calls === 1) throw new Error('close connect'); return 'ok' })
  assert.equal(out, 'ok'); assert.equal(calls, 2); assert.equal(reconnects, 1)
})

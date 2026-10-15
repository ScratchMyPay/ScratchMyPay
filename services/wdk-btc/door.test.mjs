import test from 'node:test'
import assert from 'node:assert/strict'
import { Door, FIRST_DOOR_ACCOUNT, ATTEMPT_TTL_MS, PASS_TTL_MS, MAX_PENDING } from './door.mjs'

function fakeWallet () {
  const transfers = new Map()   // index -> incoming transfers
  return {
    transfers,
    getAccount: async (index) => ({
      getAddress: async () => `tb1qdoor${index}`,
      getTransfers: async () => transfers.get(index) || [],
    }),
  }
}

test('each attempt gets its own treasury address; payment issues a 24h pass', async () => {
  let t = 1_000_000
  const w = fakeWallet()
  const door = new Door({ dbPath: ':memory:', getAccount: w.getAccount, now: () => t })
  const a = await door.start({ label: 'Gus' })
  assert.equal(a.status, 'pending')
  assert.equal(a.address, `tb1qdoor${FIRST_DOOR_ACCOUNT}`)
  assert.equal(a.amountSat, 1000)
  assert.match(a.uri, /^bitcoin:tb1qdoor1000\?amount=0\.00001000/)
  const b = await door.start()
  assert.equal(b.address, `tb1qdoor${FIRST_DOOR_ACCOUNT + 1}`)

  assert.equal((await door.check(a.id)).status, 'pending')          // nothing received yet
  w.transfers.set(FIRST_DOOR_ACCOUNT, [{ hash: 'tx1', value: 400n, height: 0 }])
  const under = await door.check(a.id)
  assert.equal(under.status, 'pending'); assert.equal(under.paidSat, 400)   // underpaid: keep waiting
  w.transfers.set(FIRST_DOOR_ACCOUNT, [{ hash: 'tx1', value: 400n, height: 0 }, { hash: 'tx2', value: 600n, height: 0 }])
  const paid = await door.check(a.id)
  assert.equal(paid.status, 'paid'); assert.equal(paid.paidSat, 1000); assert.equal(paid.confirmed, false)
  assert.ok(paid.pass.active); assert.equal(paid.pass.expiresAt, t + PASS_TTL_MS)

  w.transfers.set(FIRST_DOOR_ACCOUNT, [{ hash: 'tx1', value: 400n, height: 12 }, { hash: 'tx2', value: 600n, height: 12 }])
  assert.equal((await door.check(a.id)).confirmed, true)

  assert.deepEqual(door.revenue(), { passes: 1, activePasses: 1, revenueSat: 1000, priceSat: 1000 })
  t += PASS_TTL_MS + 1
  assert.equal(door.revenue().activePasses, 0)
  assert.equal((await door.check(a.id)).pass.active, false)
})

test('attempts expire; pending attempts are capped', async () => {
  let t = 0
  const w = fakeWallet()
  const door = new Door({ dbPath: ':memory:', getAccount: w.getAccount, now: () => t })
  const a = await door.start()
  t += ATTEMPT_TTL_MS
  assert.equal((await door.check(a.id)).status, 'expired')
  w.transfers.set(FIRST_DOOR_ACCOUNT, [{ hash: 'late', value: 1000n, height: 1 }])
  assert.equal((await door.check(a.id)).status, 'expired')          // late payment does not open the door
  for (let i = 0; i < MAX_PENDING; i++) await door.start()
  await assert.rejects(() => door.start(), RangeError)
  assert.equal(await door.check(99999), null)
})

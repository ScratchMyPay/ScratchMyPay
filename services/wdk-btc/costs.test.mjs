import test from 'node:test'
import assert from 'node:assert/strict'
import { payoutCost, coverage, RGB_PAYOUT_VBYTES } from './costs.mjs'

test('matches the measured live payout within 1 sat (197 vB at 2 sat/vB, chain charged 395)', () => {
  const c = payoutCost({ feeRates: { normal: 2 } })
  assert.equal(c.vbytes, RGB_PAYOUT_VBYTES)
  assert.equal(c.bySpeed.normal.feeSat, 394) // live tx paid 395 (wallet rounding)
  assert.equal(c.bySpeed.normal.perPayoutSat, 1394)
})

test('coverage for tonight', () => {
  const cost = payoutCost({ feeRates: { normal: 1, fast: 2 }, count: 5 })
  assert.deepEqual(cost.bySpeed.normal, { feeRateSatVb: 1, feeSat: 197, perPayoutSat: 1197, totalSat: 5985 })
  const ok = coverage({ haveSat: 10_000, cost })
  assert.equal(ok.enough, true)
  assert.equal(ok.payoutsCovered, 8)
  const low = coverage({ haveSat: 3_000, cost, speed: 'fast' })
  assert.equal(low.enough, false)
  assert.equal(low.shortSat, 5 * (2 * 197 + 1000) - 3_000)
})

test('rejects nonsense inputs', () => {
  assert.throws(() => payoutCost({ feeRates: { normal: 0 } }))
  assert.throws(() => payoutCost({ feeRates: { normal: 1 }, count: 0 }))
  assert.throws(() => payoutCost({ feeRates: { normal: 1.5 } }))
  assert.throws(() => coverage({ haveSat: -1, cost: payoutCost({ feeRates: { normal: 1 } }) }))
})

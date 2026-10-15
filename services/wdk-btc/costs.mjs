// Pure payout-cost arithmetic (unit-tested; no network).
//
// A PowerEdge RGB payout is one Bitcoin transaction. Measured on Mutinynet (2026-10-15,
// txids ea4ce78b… and 00d20ebd…): 197 vB, 1 input, 3 outputs, plus a 1,000 sat witness
// output that goes to the recipient. Cost per payout = feeRate × vbytes + witness.

export const RGB_PAYOUT_VBYTES = 197
export const RGB_WITNESS_SAT = 1000

const toInt = (value, name, { min = 0, max = Number.MAX_SAFE_INTEGER } = {}) => {
  const n = typeof value === 'bigint' ? Number(value) : Number(value)
  if (!Number.isFinite(n) || !Number.isInteger(n) || n < min || n > max) {
    throw new RangeError(`${name} must be an integer between ${min} and ${max}`)
  }
  return n
}

/** Estimated sats for `count` payouts at each fee rate (sat/vB). */
export function payoutCost ({ feeRates, vbytes = RGB_PAYOUT_VBYTES, witnessSat = RGB_WITNESS_SAT, count = 1 }) {
  const vb = toInt(vbytes, 'vbytes', { min: 1, max: 100_000 })
  const n = toInt(count, 'count', { min: 1, max: 1000 })
  const witness = toInt(witnessSat, 'witnessSat', { max: 1_000_000 })
  const out = {}
  for (const [speed, rate] of Object.entries(feeRates)) {
    const r = toInt(rate, `feeRates.${speed}`, { min: 1, max: 10_000 })
    const perPayout = r * vb + witness
    out[speed] = { feeRateSatVb: r, feeSat: r * vb, perPayoutSat: perPayout, totalSat: perPayout * n }
  }
  return { vbytes: vb, witnessSat: witness, count: n, bySpeed: out }
}

/** Can `haveSat` cover `count` payouts at the given speed? */
export function coverage ({ haveSat, cost, speed = 'normal' }) {
  const have = toInt(haveSat, 'haveSat')
  const row = cost.bySpeed[speed]
  if (!row) throw new RangeError(`unknown speed ${speed}`)
  return {
    speed,
    haveSat: have,
    needSat: row.totalSat,
    payoutsTonight: cost.count,
    payoutsCovered: Math.floor(have / row.perPayoutSat),
    enough: have >= row.totalSat,
    shortSat: Math.max(0, row.totalSat - have),
  }
}

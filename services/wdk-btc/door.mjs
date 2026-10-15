// Pay-to-enter door, on the Tether WDK treasury.
//
// An organizer buys a pass to PowerEdge by paying PRICE sats to a fresh treasury address:
// one WDK account index per attempt (so a payment is matched by address, no memo needed).
// Attempts, payments and passes live in SQLite (datadirs/door/door.sqlite3, WAL).
// Revenue (passes) arrives in the same treasury that pays the payout network fees.
//
// The wallet is injected, so the logic is unit-tested with a fake.
import fs from 'node:fs'
import path from 'node:path'
import { DatabaseSync } from 'node:sqlite'

export const DOOR_PRICE_SAT = 1000
export const ATTEMPT_TTL_MS = 15 * 60 * 1000
export const PASS_TTL_MS = 24 * 60 * 60 * 1000
export const FIRST_DOOR_ACCOUNT = 1000      // accounts 0..999 stay for the treasury itself
export const MAX_PENDING = 50

export class Door {
  /**
   * @param {object} opts
   * @param {string} opts.dbPath  SQLite file (':memory:' in tests)
   * @param {(index:number)=>Promise<{getAddress:()=>Promise<string>, getTransfers:(o?:object)=>Promise<any[]>}>} opts.getAccount
   * @param {number} [opts.priceSat]
   * @param {()=>number} [opts.now]
   */
  constructor ({ dbPath, getAccount, priceSat = DOOR_PRICE_SAT, now = Date.now }) {
    if (dbPath !== ':memory:') fs.mkdirSync(path.dirname(dbPath), { recursive: true, mode: 0o700 })
    this.db = new DatabaseSync(dbPath)
    this.db.exec(`
      PRAGMA journal_mode = WAL; PRAGMA synchronous = FULL;
      CREATE TABLE IF NOT EXISTS attempts (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        account_index INTEGER NOT NULL UNIQUE,
        address TEXT NOT NULL UNIQUE,
        amount_sat INTEGER NOT NULL,
        label TEXT NOT NULL DEFAULT '',
        created_at INTEGER NOT NULL,
        expires_at INTEGER NOT NULL,
        status TEXT NOT NULL,             -- pending | paid | expired
        txid TEXT, paid_sat INTEGER, paid_at INTEGER, confirmed INTEGER NOT NULL DEFAULT 0
      );
      CREATE TABLE IF NOT EXISTS passes (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        attempt_id INTEGER NOT NULL UNIQUE REFERENCES attempts(id),
        address TEXT NOT NULL, txid TEXT NOT NULL, paid_sat INTEGER NOT NULL,
        issued_at INTEGER NOT NULL, expires_at INTEGER NOT NULL
      );`)
    this.getAccount = getAccount
    this.priceSat = priceSat
    this.now = now
  }

  /** Create a pending attempt with its own treasury address. */
  async start ({ label = '' } = {}) {
    this.expireStale()
    const pending = this.db.prepare("SELECT COUNT(*) AS n FROM attempts WHERE status = 'pending'").get().n
    if (pending >= MAX_PENDING) throw new RangeError('too many pending door attempts; try again in a few minutes')
    const last = this.db.prepare('SELECT MAX(account_index) AS m FROM attempts').get().m
    const accountIndex = Math.max(FIRST_DOOR_ACCOUNT, (last ?? FIRST_DOOR_ACCOUNT - 1) + 1)
    const address = await (await this.getAccount(accountIndex)).getAddress()
    const t = this.now()
    const info = this.db.prepare(`INSERT INTO attempts (account_index, address, amount_sat, label, created_at, expires_at, status)
                                  VALUES (?, ?, ?, ?, ?, ?, 'pending')`)
      .run(accountIndex, address, this.priceSat, String(label).slice(0, 64), t, t + ATTEMPT_TTL_MS)
    return this.view(Number(info.lastInsertRowid))
  }

  /** Current state of an attempt; checks the chain while it is pending. */
  async check (id) {
    const row = this.db.prepare('SELECT * FROM attempts WHERE id = ?').get(id)
    if (!row) return null
    if (row.status === 'pending') {
      if (row.expires_at <= this.now()) {
        this.db.prepare("UPDATE attempts SET status = 'expired' WHERE id = ? AND status = 'pending'").run(id)
      } else {
        const incoming = await (await this.getAccount(row.account_index)).getTransfers({ direction: 'incoming' })
        const received = incoming.reduce((sum, t) => sum + Number(t.value), 0)
        if (received >= row.amount_sat) {
          const first = incoming[0]
          const t = this.now()
          const confirmed = incoming.some(x => Number(x.height) > 0) ? 1 : 0
          this.db.prepare(`UPDATE attempts SET status = 'paid', txid = ?, paid_sat = ?, paid_at = ?, confirmed = ? WHERE id = ? AND status = 'pending'`)
            .run(String(first.hash || first.txid || ''), received, t, confirmed, id)
          this.db.prepare(`INSERT OR IGNORE INTO passes (attempt_id, address, txid, paid_sat, issued_at, expires_at) VALUES (?, ?, ?, ?, ?, ?)`)
            .run(id, row.address, String(first.hash || first.txid || ''), received, t, t + PASS_TTL_MS)
        } else if (incoming.length) {
          // Underpaid so far: keep waiting, but remember what arrived.
          this.db.prepare('UPDATE attempts SET paid_sat = ? WHERE id = ?').run(received, id)
        }
      }
    } else if (row.status === 'paid' && !row.confirmed) {
      const incoming = await (await this.getAccount(row.account_index)).getTransfers({ direction: 'incoming' })
      if (incoming.some(x => Number(x.height) > 0)) this.db.prepare('UPDATE attempts SET confirmed = 1 WHERE id = ?').run(id)
    }
    return this.view(id)
  }

  view (id) {
    const a = this.db.prepare('SELECT * FROM attempts WHERE id = ?').get(id)
    if (!a) return null
    const pass = this.db.prepare('SELECT * FROM passes WHERE attempt_id = ?').get(id)
    return {
      id: a.id, status: a.status, address: a.address, amountSat: a.amount_sat, label: a.label,
      createdAt: a.created_at, expiresAt: a.expires_at, paidSat: a.paid_sat ?? 0, txid: a.txid, confirmed: !!a.confirmed,
      uri: `bitcoin:${a.address}?amount=${(a.amount_sat / 1e8).toFixed(8)}&label=PowerEdge%20pass`,
      pass: pass ? { id: pass.id, issuedAt: pass.issued_at, expiresAt: pass.expires_at, active: pass.expires_at > this.now() } : null,
    }
  }

  expireStale () {
    this.db.prepare("UPDATE attempts SET status = 'expired' WHERE status = 'pending' AND expires_at <= ?").run(this.now())
  }

  /** Revenue summary for the treasury report. */
  revenue () {
    const r = this.db.prepare("SELECT COUNT(*) AS passes, COALESCE(SUM(paid_sat), 0) AS sat FROM passes").get()
    const active = this.db.prepare('SELECT COUNT(*) AS n FROM passes WHERE expires_at > ?').get(this.now()).n
    return { passes: r.passes, activePasses: active, revenueSat: r.sat, priceSat: this.priceSat }
  }

  passes (limit = 20) {
    return this.db.prepare(`SELECT p.id, p.address, p.txid, p.paid_sat AS paidSat, p.issued_at AS issuedAt, p.expires_at AS expiresAt, a.label
                            FROM passes p JOIN attempts a ON a.id = p.attempt_id ORDER BY p.id DESC LIMIT ?`).all(limit)
  }
}

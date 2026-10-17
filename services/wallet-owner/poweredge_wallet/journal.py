"""Durable payout journal (SQLite, WAL, synchronous=FULL).

The journal, not process memory, is the source of truth for every payout. Each state
change is a compare-and-set (`UPDATE ... WHERE state = :expected`), committed and fsynced
before the next irreversible step. A send is only attempted after `SUBMITTING` is durable,
so a crash can never cause a blind second send: on restart, `SUBMITTING` becomes
`UNKNOWN_SUBMISSION` and must be reconciled against the wallet, never retried.
"""

from __future__ import annotations

import json
import logging
import sqlite3
import threading
import time
from dataclasses import dataclass
from enum import Enum
from pathlib import Path
from typing import Any

LOG = logging.getLogger("poweredge.journal")


class State(str, Enum):
    PLANNED = "PLANNED"
    APPROVED = "APPROVED"
    SUBMITTING = "SUBMITTING"
    BROADCAST = "BROADCAST"
    UNKNOWN_SUBMISSION = "UNKNOWN_SUBMISSION"
    DELIVERED = "DELIVERED"
    SETTLED = "SETTLED"
    FAILED = "FAILED"
    EXPIRED = "EXPIRED"


# Every legal transition. Anything else is a bug and is refused.
TRANSITIONS: dict[State, set[State]] = {
    State.PLANNED: {State.APPROVED, State.EXPIRED, State.FAILED},
    State.APPROVED: {State.SUBMITTING, State.EXPIRED, State.FAILED},
    State.SUBMITTING: {State.BROADCAST, State.UNKNOWN_SUBMISSION, State.FAILED},
    State.UNKNOWN_SUBMISSION: {State.BROADCAST, State.FAILED},
    State.BROADCAST: {State.DELIVERED, State.SETTLED},
    State.DELIVERED: {State.SETTLED},
    State.SETTLED: set(),
    State.FAILED: set(),
    State.EXPIRED: set(),
}

# States in which a send may already have happened: execute must never send again.
SENT_OR_MAYBE_SENT = {
    State.SUBMITTING, State.BROADCAST, State.UNKNOWN_SUBMISSION, State.DELIVERED, State.SETTLED,
}

SCHEMA = """
CREATE TABLE IF NOT EXISTS plans (
    plan_id              TEXT PRIMARY KEY,
    created_at           REAL NOT NULL,
    updated_at           REAL NOT NULL,
    state                TEXT NOT NULL,
    wallet_id            TEXT NOT NULL,
    environment          TEXT NOT NULL,
    asset_id             TEXT NOT NULL,
    asset_ticker         TEXT NOT NULL,
    recipient_id         TEXT NOT NULL,
    invoice              TEXT NOT NULL,
    amount_base_units    INTEGER NOT NULL,
    amount_display       TEXT NOT NULL,
    recipient_label      TEXT NOT NULL,
    memo                 TEXT NOT NULL,
    flow_revision        TEXT NOT NULL,
    template_digest      TEXT NOT NULL,
    plan_digest          TEXT NOT NULL,
    expiration_timestamp INTEGER NOT NULL,
    approved_by          TEXT,
    approved_at          REAL,
    idempotency_key      TEXT UNIQUE,
    txid                 TEXT,
    batch_transfer_idx   INTEGER,
    consignment_sha256   TEXT,
    consignment_path     TEXT,
    consignment_size     INTEGER,
    error                TEXT
);
CREATE TABLE IF NOT EXISTS events (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    plan_id    TEXT NOT NULL,
    at         REAL NOT NULL,
    from_state TEXT,
    to_state   TEXT NOT NULL,
    detail     TEXT
);
CREATE TABLE IF NOT EXISTS receipts (
    consignment_sha256 TEXT PRIMARY KEY,
    received_at        REAL NOT NULL,
    size               INTEGER NOT NULL,
    statuses           TEXT NOT NULL
);
"""


class JournalError(RuntimeError):
    """A journal invariant was violated (unknown plan, illegal or lost transition)."""


@dataclass(frozen=True)
class Plan:
    row: dict[str, Any]

    def __getattr__(self, name: str) -> Any:
        try:
            return self.row[name]
        except KeyError as exc:
            raise AttributeError(name) from exc

    @property
    def state_enum(self) -> State:
        return State(self.row["state"])


def _amount(display: str | None) -> float:
    try:
        return float(str(display or "0").replace(",", ""))
    except ValueError:
        return 0.0


def _percentile(values: list[float], pct: int) -> float | None:
    """Nearest-rank percentile; None when there is nothing to measure."""
    if not values:
        return None
    ordered = sorted(values)
    rank = max(1, -(-pct * len(ordered) // 100))
    return round(ordered[rank - 1], 1)


class Journal:
    def __init__(self, path: Path) -> None:
        self.path = path
        path.parent.mkdir(parents=True, exist_ok=True)
        self._lock = threading.RLock()
        self._db = sqlite3.connect(str(path), check_same_thread=False, isolation_level=None)
        self._db.row_factory = sqlite3.Row
        self._db.execute("PRAGMA journal_mode=WAL")
        self._db.execute("PRAGMA synchronous=FULL")
        self._db.execute("PRAGMA foreign_keys=ON")
        self._db.executescript(SCHEMA)

    def close(self) -> None:
        with self._lock:
            self._db.close()

    def recover(self) -> list[str]:
        """On startup: a SUBMITTING row means we died mid-send. Never retry; mark unknown."""
        with self._lock:
            rows = self._db.execute(
                "SELECT plan_id FROM plans WHERE state = ?", (State.SUBMITTING.value,)
            ).fetchall()
        recovered = []
        for row in rows:
            self.transition(
                row["plan_id"], State.SUBMITTING, State.UNKNOWN_SUBMISSION,
                detail="process restarted during submission; reconcile before any action",
            )
            recovered.append(row["plan_id"])
        if recovered:
            LOG.warning("recovered %d in-flight submission(s) as UNKNOWN_SUBMISSION", len(recovered))
        return recovered

    def recent(self, limit: int = 10) -> list[dict[str, Any]]:
        """Newest payouts first, for a dashboard table (no invoice, no local paths)."""
        with self._lock:
            rows = self._db.execute(
                "SELECT plan_id, created_at, updated_at, state, asset_ticker, amount_display, recipient_label,"
                " memo, txid, approved_by FROM plans ORDER BY created_at DESC LIMIT ?",
                (limit,),
            ).fetchall()
        return [dict(row) for row in rows]

    def stats(self, hours: int = 24, now: float | None = None) -> dict[str, Any]:
        """Per-hour payout counts and volume, plus time from approval to settled (p50/p90).

        prepared: plans created in that hour; settled / volume: by the hour they settled;
        failed: FAILED or EXPIRED, by the hour they ended. Read-only.
        """
        now = time.time() if now is None else now
        start = (int(now) // 3600 - hours + 1) * 3600
        with self._lock:
            plans = [dict(r) for r in self._db.execute(
                "SELECT plan_id, created_at, updated_at, state, asset_ticker, amount_display FROM plans"
                " WHERE updated_at >= ? OR created_at >= ?", (start, start))]
            marks = self._db.execute(
                "SELECT plan_id, to_state, MIN(at) AS at FROM events WHERE to_state IN ('APPROVED', 'SETTLED')"
                " AND at >= ? - 86400 GROUP BY plan_id, to_state", (start,)).fetchall()
        approved = {m["plan_id"]: m["at"] for m in marks if m["to_state"] == "APPROVED"}
        settled = {m["plan_id"]: m["at"] for m in marks if m["to_state"] == "SETTLED"}
        buckets = [{"hour": start + i * 3600, "prepared": 0, "settled": 0, "failed": 0, "volume": {}}
                   for i in range(hours)]

        def bucket(t: float) -> dict[str, Any] | None:
            i = int((t - start) // 3600)
            return buckets[i] if 0 <= i < hours else None

        totals = {"prepared": 0, "settled": 0, "failed": 0, "inFlight": 0, "volume": {}}
        waits = []
        for p in plans:
            if (b := bucket(p["created_at"])) is not None:
                b["prepared"] += 1
                totals["prepared"] += 1
            state = p["state"]
            if state == "SETTLED" and p["plan_id"] in settled:
                if (b := bucket(settled[p["plan_id"]])) is not None:
                    amount = _amount(p["amount_display"])
                    ticker = p["asset_ticker"] or "?"
                    b["settled"] += 1
                    b["volume"][ticker] = round(b["volume"].get(ticker, 0) + amount, 6)
                    totals["settled"] += 1
                    totals["volume"][ticker] = round(totals["volume"].get(ticker, 0) + amount, 6)
                    if p["plan_id"] in approved:
                        waits.append(settled[p["plan_id"]] - approved[p["plan_id"]])
            elif state in ("FAILED", "EXPIRED"):
                if (b := bucket(p["updated_at"])) is not None:
                    b["failed"] += 1
                    totals["failed"] += 1
            elif state in ("APPROVED", "SUBMITTING", "BROADCAST", "DELIVERED", "UNKNOWN_SUBMISSION"):
                totals["inFlight"] += 1
        return {
            "since": start,
            "hours": hours,
            "buckets": buckets,
            "totals": totals,
            "settleSeconds": {"n": len(waits), "p50": _percentile(waits, 50), "p90": _percentile(waits, 90)},
        }

    def in_flight(self, max_age_seconds: float = 86_400) -> list[str]:
        """Payouts that left (or may have left) the wallet and are not settled yet, newest first.

        Bounded by age so an old payout that will never settle does not get refreshed forever.
        """
        states = (State.UNKNOWN_SUBMISSION.value, State.BROADCAST.value, State.DELIVERED.value)
        with self._lock:
            rows = self._db.execute(
                "SELECT plan_id FROM plans WHERE state IN (?, ?, ?) AND updated_at > ? ORDER BY updated_at DESC",
                (*states, time.time() - max_age_seconds),
            ).fetchall()
        return [row["plan_id"] for row in rows]

    def insert_plan(self, fields: dict[str, Any]) -> Plan:
        now = time.time()
        row = {**fields, "created_at": now, "updated_at": now, "state": State.PLANNED.value}
        columns = ", ".join(row)
        marks = ", ".join(f":{k}" for k in row)
        with self._lock:
            self._db.execute("BEGIN IMMEDIATE")
            try:
                self._db.execute(f"INSERT INTO plans ({columns}) VALUES ({marks})", row)
                self._event(row["plan_id"], None, State.PLANNED, "plan created")
                self._db.execute("COMMIT")
            except Exception:
                self._db.execute("ROLLBACK")
                raise
        return self.get(row["plan_id"])

    def get(self, plan_id: str) -> Plan:
        with self._lock:
            row = self._db.execute("SELECT * FROM plans WHERE plan_id = ?", (plan_id,)).fetchone()
        if row is None:
            raise JournalError(f"unknown plan {plan_id}")
        return Plan(dict(row))

    def find_by_idempotency_key(self, key: str) -> Plan | None:
        with self._lock:
            row = self._db.execute(
                "SELECT * FROM plans WHERE idempotency_key = ?", (key,)
            ).fetchone()
        return Plan(dict(row)) if row else None

    def transition(
        self, plan_id: str, expected: State, new: State, *, detail: str = "", **fields: Any
    ) -> Plan:
        """Compare-and-set state change, durable on return."""
        if new not in TRANSITIONS[expected]:
            raise JournalError(f"illegal transition {expected.value} -> {new.value}")
        assignments = {"state": new.value, "updated_at": time.time(), **fields}
        set_clause = ", ".join(f"{k} = :{k}" for k in assignments)
        params = {**assignments, "plan_id": plan_id, "expected": expected.value}
        with self._lock:
            self._db.execute("BEGIN IMMEDIATE")
            try:
                cursor = self._db.execute(
                    f"UPDATE plans SET {set_clause} WHERE plan_id = :plan_id AND state = :expected",
                    params,
                )
                if cursor.rowcount != 1:
                    raise JournalError(
                        f"plan {plan_id} is not in state {expected.value}; transition refused"
                    )
                self._event(plan_id, expected, new, detail)
                self._db.execute("COMMIT")
            except Exception:
                self._db.execute("ROLLBACK")
                raise
        LOG.info("plan %s: %s -> %s %s", plan_id, expected.value, new.value, detail)
        return self.get(plan_id)

    def events(self, plan_id: str) -> list[dict[str, Any]]:
        with self._lock:
            rows = self._db.execute(
                "SELECT at, from_state, to_state, detail FROM events WHERE plan_id = ? ORDER BY id",
                (plan_id,),
            ).fetchall()
        return [dict(r) for r in rows]

    def record_receipt(self, sha256: str, size: int, statuses: dict[int, str]) -> None:
        with self._lock:
            self._db.execute(
                "INSERT OR REPLACE INTO receipts (consignment_sha256, received_at, size, statuses)"
                " VALUES (?, ?, ?, ?)",
                (sha256, time.time(), size, json.dumps(statuses, sort_keys=True)),
            )

    def receipt(self, sha256: str) -> dict[str, Any] | None:
        with self._lock:
            row = self._db.execute(
                "SELECT * FROM receipts WHERE consignment_sha256 = ?", (sha256,)
            ).fetchone()
        if row is None:
            return None
        out = dict(row)
        out["statuses"] = json.loads(out["statuses"])
        return out

    def note(self, ref: str, detail: str) -> None:
        """Durable free-form entry outside the plan state machine (e.g. a plain-sats payment)."""
        with self._lock:
            self._db.execute(
                "INSERT INTO events (plan_id, at, from_state, to_state, detail) VALUES (?, ?, NULL, 'NOTE', ?)",
                (ref, time.time(), detail),
            )
            self._db.commit()

    def _event(self, plan_id: str, old: State | None, new: State, detail: str) -> None:
        self._db.execute(
            "INSERT INTO events (plan_id, at, from_state, to_state, detail) VALUES (?, ?, ?, ?, ?)",
            (plan_id, time.time(), old.value if old else None, new.value, detail),
        )

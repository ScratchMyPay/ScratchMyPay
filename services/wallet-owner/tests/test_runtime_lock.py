"""rgb-lib's runtime lock: removed only when provably stale."""
from __future__ import annotations

import fcntl
from pathlib import Path

from poweredge_wallet.__main__ import clear_stale_runtime_lock


def _wallet(tmp_path: Path) -> Path:
    data_dir = tmp_path / "datadirs" / "recipient"
    (data_dir / "20126ed0").mkdir(parents=True)
    (data_dir / "20126ed0" / "rgb_runtime.lock").touch()
    return data_dir


def test_stale_runtime_lock_is_removed_when_no_process_owns_the_wallet(tmp_path):
    data_dir = _wallet(tmp_path)
    removed = clear_stale_runtime_lock(data_dir)
    assert [p.name for p in removed] == ["rgb_runtime.lock"]
    assert not (data_dir / "20126ed0" / "rgb_runtime.lock").exists()


def test_runtime_lock_is_left_alone_while_another_process_owns_the_wallet(tmp_path):
    data_dir = _wallet(tmp_path)
    owner = (data_dir.parent / ".recipient.rgb-bill.lock").open("a+")
    fcntl.flock(owner.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)   # a running wallet process (Bill's lock)
    try:
        assert clear_stale_runtime_lock(data_dir) == []
        assert (data_dir / "20126ed0" / "rgb_runtime.lock").exists()
    finally:
        owner.close()


def test_nothing_to_do_without_a_runtime_lock(tmp_path):
    data_dir = tmp_path / "datadirs" / "payer"
    data_dir.mkdir(parents=True)
    assert clear_stale_runtime_lock(data_dir) == []

#!/usr/bin/env python3
"""Provision one fresh Mutinynet rgb-lib wallet for PowerEdge without printing its mnemonic.

Adapted from EdwinKestler/bill@be3d237 `scripts/rgb_provision.py` (prior work, same author):
same rgb-lib 0.3.0b7 calls, but PowerEdge paths and roles. The generated env file uses the
exact keys that Bill's `web.rgb.config.RgbSettings.from_env` reads, so the wallet-owner
sidecar can reuse Bill's adapter unchanged.

Roles: payer -> Bill role A (sends), recipient -> Bill role B (creates invoices, imports).
Writes (all git-ignored): datadirs/<role>/, secrets/<role>.mnemonic, .env.<role>, .rgb-oob/<role>/.
Refuses to overwrite anything. Test network only (custom signet / Mutinynet).
"""

from __future__ import annotations

import argparse
import logging
import os
import shutil
import stat
import sys
from pathlib import Path

import rgb_lib

LOG = logging.getLogger("poweredge.provision")
ROOT = Path(__file__).resolve().parents[3]
ROLES = {"payer": "A", "recipient": "B"}


def _paths(role: str) -> dict[str, Path]:
    return {
        "data_dir": ROOT / "datadirs" / role,
        "mnemonic_file": ROOT / "secrets" / f"{role}.mnemonic",
        "env_file": ROOT / f".env.{role}",
        "inbox": ROOT / ".rgb-oob" / role,
    }


def _write_private(path: Path, contents: str) -> None:
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
            handle.write(contents)
            handle.flush()
            os.fsync(handle.fileno())
    except Exception:
        path.unlink(missing_ok=True)
        raise


def _private_dir(path: Path) -> None:
    path.mkdir(mode=0o700, parents=True, exist_ok=True)
    os.chmod(path, 0o700)


def _env_contents(role: str, letter: str, fingerprint: str, paths: dict[str, Path]) -> str:
    payer, recipient = _paths("payer"), _paths("recipient")
    return "\n".join(
        [
            f"BILL_ROLE={letter}",
            f"POWEREDGE_ROLE={role}",
            "POWEREDGE_NETWORK=mutinynet",
            f"RGB_DATA_DIR_A={payer['data_dir']}",
            f"RGB_DATA_DIR_B={recipient['data_dir']}",
            f"RGB_MASTER_FINGERPRINT_{letter}={fingerprint}",
            f"RGB_MNEMONIC_FILE_{letter}={paths['mnemonic_file']}",
            "ELECTRUM_URL=https://mutinynet.com/api",
            "RGB_OOB_CONSIGNMENT=1",
            "RGB_BOOTSTRAP_PROXY=rpcs://proxy.kaleidoswap.com/json-rpc",
            f"RGB_OOB_INBOX_{letter}={paths['inbox']}",
            "RGB_MIN_CONFIRMATIONS=1",
            "RGB_FEE_RATE=2",
            "RGB_WITNESS_AMOUNT_SAT=1000",
            "RGB_MAX_CONSIGNMENT_BYTES=655360",
            "RGB_ASSET_ID=",
            "RGB_ASSET_PRECISION=6",
            "",
        ]
    )


def provision(role: str) -> tuple[dict[str, Path], str]:
    """Create the wallet; return its paths and a funding address (never the mnemonic)."""
    if role not in ROLES:
        raise ValueError(f"role must be one of {sorted(ROLES)}")
    letter = ROLES[role]
    paths = _paths(role)
    for key in ("data_dir", "mnemonic_file", "env_file"):
        if paths[key].exists():
            raise FileExistsError(f"refusing to overwrite existing path: {paths[key]}")

    for parent in (paths["data_dir"].parent, paths["mnemonic_file"].parent, paths["inbox"]):
        _private_dir(parent)

    keys = rgb_lib.generate_keys(
        rgb_lib.BitcoinNetwork.SIGNET_CUSTOM,
        rgb_lib.WitnessVersion.TAPROOT,
    )
    paths["data_dir"].mkdir(mode=0o700)
    try:
        wallet_data = rgb_lib.WalletData(
            data_dir=str(paths["data_dir"]),
            bitcoin_network=rgb_lib.BitcoinNetwork.SIGNET_CUSTOM,
            database_type=rgb_lib.DatabaseType.SQLITE,
            max_allocations_per_utxo=5,
            supported_schemas=[
                rgb_lib.AssetSchema.NIA,
                rgb_lib.AssetSchema.CFA,
                rgb_lib.AssetSchema.IFA,
                rgb_lib.AssetSchema.UDA,
            ],
        )
        singlesig_keys = rgb_lib.SinglesigKeys(
            account_xpub_vanilla=keys.account_xpub_vanilla,
            account_xpub_colored=keys.account_xpub_colored,
            vanilla_keychain=None,
            master_fingerprint=keys.master_fingerprint,
            mnemonic=keys.mnemonic,
            witness_version=rgb_lib.WitnessVersion.TAPROOT,
        )
        wallet = rgb_lib.Wallet(wallet_data, singlesig_keys)
        if wallet.get_wallet_data().bitcoin_network != rgb_lib.BitcoinNetwork.SIGNET_CUSTOM:
            raise RuntimeError("created wallet is not custom signet (Mutinynet)")
        address = wallet.get_address()
        _write_private(paths["mnemonic_file"], keys.mnemonic + "\n")
        _write_private(
            paths["env_file"], _env_contents(role, letter, keys.master_fingerprint, paths)
        )
        del wallet
    except Exception:
        paths["mnemonic_file"].unlink(missing_ok=True)
        paths["env_file"].unlink(missing_ok=True)
        shutil.rmtree(paths["data_dir"])
        raise
    finally:
        del keys  # never log or print the mnemonic

    return paths, address


def main() -> None:
    logging.basicConfig(level=logging.INFO, format="%(levelname)s %(name)s: %(message)s")
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--role", required=True, choices=sorted(ROLES))
    args = parser.parse_args()
    try:
        paths, address = provision(args.role)
    except Exception as exc:
        LOG.error("provision failed: %s", exc)
        raise SystemExit(1) from exc

    mode = stat.S_IMODE(paths["mnemonic_file"].stat().st_mode)
    LOG.info("provisioned %s wallet (Mutinynet, Bill role %s)", args.role, ROLES[args.role])
    LOG.info("data_dir=%s", paths["data_dir"])
    LOG.info("mnemonic_file=%s mode=%04o (contents hidden)", paths["mnemonic_file"], mode)
    LOG.info("env_file=%s", paths["env_file"])
    print(f"funding_address={address}")


if __name__ == "__main__":
    sys.exit(main())

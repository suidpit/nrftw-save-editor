# /// script
# requires-python = ">=3.12"
# dependencies = []
# ///
"""
Regenerate the repo's reproducible static/generated assets.

By default this rebuilds:
- public/catalog.db
- src/lib/data/all_unique_item_enchantments.json

It does not rebuild:
- public/cerimal_zstd.dict (manual IDA extraction)

Optionally, pass --with-wasm to also rebuild src/wasm-pkg via wasm-pack.

Usage:
    uv run --script scripts/regenerate_static_assets.py
    uv run --script scripts/regenerate_static_assets.py --with-wasm
"""

from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_BUNDLE_DIR = ROOT / "dataDir" / "StreamingAssets" / "aa" / "StandaloneWindows64"
DEFAULT_BUNDLE = DEFAULT_BUNDLE_DIR / "qdb_assets_all_e4d83d504b7b9074accd1297011f22ec.bundle"
DEFAULT_MONO = DEFAULT_BUNDLE_DIR / "96f91f0ffa73a2e48b992373b0be129e_monoscripts_48ec820bfc8e835fc2a09db0b8cb63f7.bundle"
DEFAULT_WORLD = DEFAULT_BUNDLE_DIR / "world_assets_all_afa19d7710b311abc0c1e70f5c9a9344.bundle"
DEFAULT_DB = ROOT / "public" / "catalog.db"
DEFAULT_RAW_DB = ROOT / "public" / "catalog.raw.db"


def require_file(path: Path, label: str) -> None:
    if not path.is_file():
        raise SystemExit(f"Missing {label}: {path}")


def run_step(title: str, cmd: list[str], cwd: Path | None = None) -> None:
    print(f"\n==> {title}", flush=True)
    print(" ".join(cmd), flush=True)
    subprocess.run(cmd, cwd=cwd or ROOT, check=True)


def main() -> None:
    parser = argparse.ArgumentParser(description="Regenerate static assets for the repo")
    parser.add_argument("--bundle", type=Path, default=DEFAULT_BUNDLE, help="Path to qdb_assets_all bundle")
    parser.add_argument("--mono", type=Path, default=DEFAULT_MONO, help="Path to monoscripts bundle")
    parser.add_argument("--world", type=Path, default=DEFAULT_WORLD, help="Path to world assets bundle")
    parser.add_argument("--db", type=Path, default=DEFAULT_DB, help="Path to output catalog.db")
    parser.add_argument("--raw-db", type=Path, default=DEFAULT_RAW_DB, help="Path to intermediate raw catalog DB")
    parser.add_argument("--offline-site-cache", action="store_true", help="Build catalog 2.0 from archived site pages only")
    parser.add_argument("--with-wasm", action="store_true", help="Also rebuild src/wasm-pkg with wasm-pack")
    args = parser.parse_args()

    uv_bin = shutil.which("uv")
    if uv_bin is None:
        raise SystemExit("Missing required command: uv")

    require_file(args.bundle, "bundle")
    require_file(args.mono, "monoscripts bundle")
    require_file(args.world, "world assets bundle")
    args.db.parent.mkdir(parents=True, exist_ok=True)

    run_step(
        "Rebuild raw catalog.db",
        [
            uv_bin,
            "run",
            "--script",
            str(ROOT / "scripts" / "bundle_catalog.py"),
            "--bundle",
            str(args.bundle),
            "--db",
            str(args.raw_db),
        ],
    )
    build_catalog_cmd = [
        uv_bin,
        "run",
        "--script",
        str(ROOT / "scripts" / "build_catalog_v2.py"),
        "--input-db",
        str(args.raw_db),
        "--output-db",
        str(args.db),
    ]
    if args.offline_site_cache:
        build_catalog_cmd.append("--offline")
    run_step(
        "Build catalog 2.0",
        build_catalog_cmd,
    )
    run_step(
        "Rebuild runtime unique item presets",
        [
            uv_bin,
            "run",
            "--script",
            str(ROOT / "scripts" / "generate_unique_item_presets.py"),
            "--bundle",
            str(args.bundle),
            "--mono",
            str(args.mono),
            "--world",
            str(args.world),
            "--db",
            str(args.db),
        ],
    )

    if args.with_wasm:
        wasm_pack_bin = shutil.which("wasm-pack")
        if wasm_pack_bin is None:
            raise SystemExit("Missing required command for --with-wasm: wasm-pack")
        run_step(
            "Rebuild src/wasm-pkg",
            [wasm_pack_bin, "build", "--target", "web", "--out-dir", "../src/wasm-pkg"],
            cwd=ROOT / "wasm",
        )

    print("\nDone.", flush=True)
    print(f"- catalog: {args.db}", flush=True)
    print(f"- raw catalog: {args.raw_db}", flush=True)
    print(f"- unique mapping: {ROOT / 'src' / 'lib' / 'data' / 'all_unique_item_enchantments.json'}", flush=True)
    print("- zstd dictionary: unchanged (manual IDA extraction)", flush=True)


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as exc:
        sys.exit(exc.returncode)

"""
Parse Unity asset bundles and build a SQLite catalog mapping AssetGuid → item info.

Usage:
    uv run python bundle_catalog.py [--bundle BUNDLE] [--db DB] [--query GUID]

Default bundle: qdb_assets_all (contains all item/armament definitions).
Default db:     catalog.db
"""

import argparse
import json
import re
import sqlite3
import sys
from io import BytesIO
from pathlib import Path

from PIL import Image
from UnityPy.environment import Environment

BUNDLE_DIR = (
    Path(__file__).parent
    / "resources/game_dir/NoRestForTheWicked_Data/StreamingAssets/aa/StandaloneWindows64"
)
DEFAULT_BUNDLE = "qdb_assets_all_e4d83d504b7b9074accd1297011f22ec.bundle"
MONOSCRIPTS_BUNDLE = "96f91f0ffa73a2e48b992373b0be129e_monoscripts_48ec820bfc8e835fc2a09db0b8cb63f7.bundle"
WORLD_ASSETS_BUNDLE = "world_assets_all_afa19d7710b311abc0c1e70f5c9a9344.bundle"
DEFAULT_DB = Path(__file__).parent.parent / "public" / "catalog.db"


def camel_to_display(name: str) -> str:
    """Convert camelCase/PascalCase to 'Title Case With Spaces'."""
    # Insert space before uppercase letters that follow lowercase letters or digits
    spaced = re.sub(r"(?<=[a-z0-9])(?=[A-Z])", " ", name)
    # Insert space before sequences of uppercase followed by lowercase (e.g. "HTMLParser")
    spaced = re.sub(r"(?<=[A-Z])(?=[A-Z][a-z])", " ", spaced)
    return spaced.title()


def serialize(obj, depth: int = 0) -> object:
    """Recursively convert a UnityPy object/value to a JSON-serialisable form."""
    if depth > 8:
        return None
    if obj is None or isinstance(obj, (bool, int, float, str)):
        return obj
    if isinstance(obj, bytes):
        return obj.hex()
    if isinstance(obj, list):
        return [serialize(v, depth + 1) for v in obj]
    if isinstance(obj, dict):
        return {k: serialize(v, depth + 1) for k, v in obj.items()}
    # PPtr
    if hasattr(obj, "m_FileID") and hasattr(obj, "m_PathID"):
        return {"file_id": obj.m_FileID, "path_id": obj.m_PathID}
    # UnknownObject / arbitrary UnityPy object — walk __dict__
    if hasattr(obj, "__dict__"):
        result = {}
        for k, v in obj.__dict__.items():
            if k in ("object_reader", "assets_file", "assetsfile"):
                continue
            result[k] = serialize(v, depth + 1)
        return result
    return repr(obj)


def build_catalog(bundle_path: Path, db_path: Path) -> None:
    print(f"Loading bundle: {bundle_path.name} …", flush=True)
    extra_bundles = []
    for fname in (MONOSCRIPTS_BUNDLE, WORLD_ASSETS_BUNDLE):
        p = bundle_path.parent / fname
        if p.exists():
            extra_bundles.append(str(p))
            print(f"Also loading: {fname} …", flush=True)
    env = Environment(str(bundle_path), *extra_bundles)
    bundle_name = bundle_path.name

    con = sqlite3.connect(db_path)
    con.execute("""
        CREATE TABLE IF NOT EXISTS assets (
            asset_guid   INTEGER PRIMARY KEY,
            name         TEXT NOT NULL,
            display_name TEXT NOT NULL,
            bundle       TEXT NOT NULL,
            data         TEXT,
            script_type  TEXT,
            icon_png     BLOB
        )
    """)
    con.execute("CREATE INDEX IF NOT EXISTS idx_name ON assets(name)")
    con.execute("CREATE INDEX IF NOT EXISTS idx_display ON assets(display_name)")
    for col, col_def in [("script_type", "TEXT"), ("icon_png", "BLOB")]:
        try:
            con.execute(f"ALTER TABLE assets ADD COLUMN {col} {col_def}")
        except sqlite3.OperationalError:
            pass  # column already exists
    con.execute("CREATE INDEX IF NOT EXISTS idx_script_type ON assets(script_type)")

    inserted = updated = skipped = 0

    for obj in env.objects:
        if obj.type.name != "MonoBehaviour":
            continue
        try:
            data = obj.read()
        except Exception:
            skipped += 1
            continue

        guid_obj = getattr(data, "AssetGuid", None)
        if guid_obj is None:
            skipped += 1
            continue
        guid_val = getattr(guid_obj, "Value", None)
        if guid_val is None or guid_val == 0:
            skipped += 1
            continue

        name = getattr(data, "m_Name", "") or ""
        if not name:
            skipped += 1
            continue

        display_name = camel_to_display(name)

        # Resolve m_Script PPtr to get the Unity class name
        script_type = None
        script_pptr = getattr(data, "m_Script", None)
        if script_pptr is not None:
            try:
                script_obj = script_pptr.read()
                script_type = getattr(script_obj, "m_ClassName", None) or getattr(
                    script_obj, "m_Name", None
                )
            except Exception:
                pass

        # Serialise all fields except the basics
        extra = {}
        for k, v in data.__dict__.items():
            if k in (
                "m_Enabled",
                "m_GameObject",
                "m_Name",
                "m_Script",
                "AssetGuid",
                "object_reader",
            ):
                continue
            extra[k] = serialize(v)

        data_json = json.dumps(extra) if extra else None

        # Extract icon thumbnail (64×64 PNG) from ItemIcon PPtr (local or external)
        icon_png = None
        icon_pptr = getattr(data, "ItemIcon", None)
        if (
            icon_pptr is not None
            and hasattr(icon_pptr, "m_PathID")
            and icon_pptr.m_PathID != 0
        ):
            try:
                img = icon_pptr.read().image
                img.thumbnail((64, 64), Image.LANCZOS)
                buf = BytesIO()
                img.save(buf, format="PNG", optimize=True)
                icon_png = buf.getvalue()
            except Exception:
                pass

        existing = con.execute(
            "SELECT asset_guid FROM assets WHERE asset_guid = ?", (guid_val,)
        ).fetchone()
        if existing:
            con.execute(
                "UPDATE assets SET name=?, display_name=?, bundle=?, data=?, script_type=?, icon_png=? WHERE asset_guid=?",
                (
                    name,
                    display_name,
                    bundle_name,
                    data_json,
                    script_type,
                    icon_png,
                    guid_val,
                ),
            )
            updated += 1
        else:
            con.execute(
                "INSERT INTO assets (asset_guid, name, display_name, bundle, data, script_type, icon_png) VALUES (?,?,?,?,?,?,?)",
                (
                    guid_val,
                    name,
                    display_name,
                    bundle_name,
                    data_json,
                    script_type,
                    icon_png,
                ),
            )
            inserted += 1

    # Propagate icons within same-name groups: assets that share a name but have no
    # ItemIcon field (e.g. ArmamentDataAsset) inherit the icon from a sibling that does.
    propagated = con.execute("""
        UPDATE assets
        SET icon_png = (
            SELECT b.icon_png FROM assets b
            WHERE b.name = assets.name AND b.icon_png IS NOT NULL
            LIMIT 1
        )
        WHERE icon_png IS NULL
          AND EXISTS (
            SELECT 1 FROM assets b
            WHERE b.name = assets.name AND b.icon_png IS NOT NULL
          )
    """).rowcount
    print(f"Icon propagation: {propagated} assets updated", flush=True)

    con.commit()
    con.close()
    print(
        f"Done. inserted={inserted}, updated={updated}, skipped={skipped} → {db_path}"
    )


def query_catalog(db_path: Path, guid: int) -> None:
    if not db_path.exists():
        print(f"Database not found: {db_path}", file=sys.stderr)
        sys.exit(1)
    con = sqlite3.connect(db_path)
    row = con.execute(
        "SELECT asset_guid, name, display_name, bundle, data, script_type FROM assets WHERE asset_guid = ?",
        (guid,),
    ).fetchone()
    con.close()
    if row is None:
        print(f"GUID {guid} not found in catalog.")
        return
    asset_guid, name, display_name, bundle, data_json, script_type = row
    print(f"asset_guid  : {asset_guid}")
    print(f"name        : {name}")
    print(f"display_name: {display_name}")
    print(f"bundle      : {bundle}")
    print(f"script_type : {script_type}")
    if data_json:
        data = json.loads(data_json)
        print("data:")
        for k, v in data.items():
            print(f"  {k}: {json.dumps(v)[:120]}")


def main() -> None:
    parser = argparse.ArgumentParser(description="Build/query the asset GUID catalog")
    parser.add_argument(
        "--bundle",
        default=str(BUNDLE_DIR / DEFAULT_BUNDLE),
        help="Path to the Unity asset bundle to parse",
    )
    parser.add_argument(
        "--db",
        default=str(DEFAULT_DB),
        help="Path to the SQLite database (created if absent)",
    )
    parser.add_argument(
        "--query",
        type=lambda x: int(x, 0),
        metavar="GUID",
        help="Look up a single AssetGuid value in the database",
    )
    args = parser.parse_args()

    db_path = Path(args.db)

    if args.query is not None:
        query_catalog(db_path, args.query)
        return

    build_catalog(Path(args.bundle), db_path)


if __name__ == "__main__":
    main()

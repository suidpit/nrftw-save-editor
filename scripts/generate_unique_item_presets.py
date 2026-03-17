# /// script
# requires-python = ">=3.12"
# dependencies = [
#     "UnityPy",
# ]
# ///
"""
Generate the runtime unique-item preset mapping used by the frontend.

This scans the game bundles, resolves item/enchantment AssetGuids, applies a
small built-in fallback heuristic for mismatched internal folder names, and
writes src/lib/data/all_unique_item_enchantments.json.

Usage:
    uv run --script scripts/generate_unique_item_presets.py
"""

from __future__ import annotations

import argparse
import json
import re
import sqlite3
from dataclasses import dataclass
from pathlib import Path

from UnityPy.environment import Environment

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_BUNDLE_DIR = ROOT / "dataDir" / "StreamingAssets" / "aa" / "StandaloneWindows64"
DEFAULT_BUNDLE = DEFAULT_BUNDLE_DIR / "qdb_assets_all_e4d83d504b7b9074accd1297011f22ec.bundle"
DEFAULT_MONO = DEFAULT_BUNDLE_DIR / "96f91f0ffa73a2e48b992373b0be129e_monoscripts_48ec820bfc8e835fc2a09db0b8cb63f7.bundle"
DEFAULT_WORLD = DEFAULT_BUNDLE_DIR / "world_assets_all_afa19d7710b311abc0c1e70f5c9a9344.bundle"
DEFAULT_DB = ROOT / "public" / "catalog.db"
DEFAULT_OUT = ROOT / "src" / "lib" / "data" / "all_unique_item_enchantments.json"

ENTITY_ITEM_PAT = re.compile(r"Assets/quantumAssetDatabase/entities/items/.*/unique/([^/]+)\.asset")
ARMAMENT_ITEM_PAT = re.compile(r"Assets/armaments/root/.*/([^/]+)/\1\.asset$")
ENCH_PAT = re.compile(r"Assets/quantumAssetDatabase/enchantments/(?:.*/)?unique/(?:.*/)?([^/]+)/([^/]+)\.assett?")

PREFERRED_ITEM_TYPES = (
    "WeaponStaticDataAsset",
    "RingsDataAsset",
    "HelmDataAsset",
    "BodyDataAsset",
    "PantsDataAsset",
    "GlovesDataAsset",
)

ITEM_TYPES = (
    "WeaponStaticDataAsset",
    "ArmamentDataAsset",
    "HelmDataAsset",
    "BodyDataAsset",
    "PantsDataAsset",
    "GlovesDataAsset",
    "RingsDataAsset",
)

# Some bundle folders contain generic unique enchantments alongside the item's
# actual live unique set. Keep a narrow override list for cases where the
# runtime/source-of-truth item data disagrees with the folder-wide scan.
ENCHANTMENT_NAME_OVERRIDES: dict[str, tuple[str, ...]] = {
    "cinderAndStone": (
        "cinderAndStone0",
        "cinderAndStone1",
        "cinderAndStone2",
    ),
}


@dataclass
class AssetRow:
    guid: int
    name: str
    display_name: str
    script_type: str


@dataclass
class EnchantmentStem:
    stem: str
    normalized_stem: str
    entries: list[AssetRow]
    indexes: list[int]


def script_type(data) -> str | None:
    script_pptr = getattr(data, "m_Script", None)
    if script_pptr is None:
        return None
    try:
        script_obj = script_pptr.read()
    except Exception:
        return None
    return getattr(script_obj, "m_ClassName", None) or getattr(script_obj, "m_Name", None)


def asset_guid(data) -> int | None:
    guid_obj = getattr(data, "AssetGuid", None)
    if guid_obj is None:
        return None
    return getattr(guid_obj, "Value", None)


def normalize_name(value: str) -> str:
    return re.sub(r"[^a-z0-9]", "", value.lower())


def trailing_number(value: str) -> int | None:
    match = re.search(r"(\d+)$", value)
    return int(match.group(1)) if match else None


def strip_trailing_number(value: str) -> str:
    return re.sub(r"\d+$", "", value)


def levenshtein(a: str, b: str) -> int:
    if a == b:
        return 0
    if not a:
        return len(b)
    if not b:
        return len(a)

    prev = list(range(len(b) + 1))
    for i, ca in enumerate(a, start=1):
        curr = [i]
        for j, cb in enumerate(b, start=1):
            curr.append(
                min(
                    curr[j - 1] + 1,
                    prev[j] + 1,
                    prev[j - 1] + (ca != cb),
                )
            )
        prev = curr
    return prev[-1]


def common_prefix_len(a: str, b: str) -> int:
    size = min(len(a), len(b))
    for i in range(size):
        if a[i] != b[i]:
            return i
    return size


def load_assets(con: sqlite3.Connection, script_types: tuple[str, ...]) -> list[AssetRow]:
    placeholders = ",".join("?" for _ in script_types)
    rows = con.execute(
        f"""
        SELECT asset_guid, name, display_name, script_type
        FROM assets
        WHERE script_type IN ({placeholders})
        ORDER BY script_type, name
        """,
        script_types,
    ).fetchall()
    return [
        AssetRow(
            guid=int(row[0]),
            name=str(row[1]),
            display_name=str(row[2]),
            script_type=str(row[3]),
        )
        for row in rows
    ]


def build_enchantment_stems(enchantments: list[AssetRow]) -> list[EnchantmentStem]:
    grouped: dict[str, list[AssetRow]] = {}
    for row in enchantments:
        index = trailing_number(row.name)
        if index is None:
            continue
        grouped.setdefault(strip_trailing_number(row.name), []).append(row)

    stems: list[EnchantmentStem] = []
    for stem, rows in grouped.items():
        sorted_rows = sorted(rows, key=lambda row: trailing_number(row.name) or 0)
        stems.append(
            EnchantmentStem(
                stem=stem,
                normalized_stem=normalize_name(stem),
                entries=sorted_rows,
                indexes=[trailing_number(row.name) or 0 for row in sorted_rows],
            )
        )
    return stems


def score_candidate(item: AssetRow, stem: EnchantmentStem) -> float:
    item_norm = normalize_name(item.name)
    stem_norm = stem.normalized_stem
    distance = levenshtein(item_norm, stem_norm)
    prefix = common_prefix_len(item_norm, stem_norm)
    max_len = max(len(item_norm), len(stem_norm), 1)
    similarity = 1.0 - (distance / max_len)
    prefix_ratio = prefix / max_len
    contiguous = stem.indexes == list(range(stem.indexes[0], stem.indexes[0] + len(stem.indexes)))
    starts_at_zero_or_one = bool(stem.indexes) and stem.indexes[0] in (0, 1)
    score = similarity + prefix_ratio
    if contiguous:
        score += 0.25
    if starts_at_zero_or_one:
        score += 0.15
    if len(stem.entries) >= 3:
        score += 0.1
    return score


def build_named_candidates(con: sqlite3.Connection) -> dict[str, set[str]]:
    items = load_assets(con, ITEM_TYPES)
    enchantments = load_assets(con, ("EnchantmentDataAsset",))
    stems = build_enchantment_stems(enchantments)

    candidates: dict[str, set[str]] = {}
    for item in items:
        ranked = [(score_candidate(item, stem), stem) for stem in stems]
        ranked = [entry for entry in ranked if entry[0] >= 1.4]
        if not ranked:
            continue
        ranked.sort(key=lambda entry: (entry[0], len(entry[1].entries)), reverse=True)
        best_score, best_stem = ranked[0]
        if best_score < 1.55:
            continue
        if item.script_type != "WeaponStaticDataAsset":
            continue
        candidates[item.name] = {row.name for row in best_stem.entries}
    return candidates


def choose_base_item(con: sqlite3.Connection, name: str) -> dict[str, object] | None:
    rows = con.execute(
        """
        SELECT asset_guid, name, display_name, script_type
        FROM assets
        WHERE name = ?
        ORDER BY script_type
        """,
        (name,),
    ).fetchall()
    normalized = [
        {
            "guid": str(int(row[0])),
            "name": str(row[1]),
            "displayName": str(row[2]),
            "scriptType": str(row[3]) if row[3] is not None else None,
        }
        for row in rows
    ]
    for preferred in PREFERRED_ITEM_TYPES:
        for row in normalized:
            if row["scriptType"] == preferred:
                return row
    return normalized[0] if normalized else None


def enrich_name(con: sqlite3.Connection, entry: dict[str, object]) -> dict[str, object]:
    row = con.execute(
        "SELECT display_name, script_type FROM assets WHERE asset_guid = ?",
        (int(entry["guid"]),),
    ).fetchone()
    if row is not None:
        entry["displayName"] = str(row[0])
        entry["scriptType"] = str(row[1]) if row[1] is not None else entry.get("scriptType")
    return entry


def apply_enchantment_name_overrides(enchantments_by_folder: dict[str, list[dict[str, object]]]) -> None:
    for folder, desired_names in ENCHANTMENT_NAME_OVERRIDES.items():
        entries = enchantments_by_folder.get(folder)
        if not entries:
            continue

        by_name = {str(entry["name"]): entry for entry in entries}
        missing = [name for name in desired_names if name not in by_name]
        if missing:
            raise SystemExit(
                f"Missing override enchantments for {folder}: {', '.join(missing)}"
            )
        enchantments_by_folder[folder] = [by_name[name] for name in desired_names]


def main() -> None:
    parser = argparse.ArgumentParser(description="Generate runtime unique item preset mapping")
    parser.add_argument("--bundle", type=Path, default=DEFAULT_BUNDLE)
    parser.add_argument("--mono", type=Path, default=DEFAULT_MONO)
    parser.add_argument("--world", type=Path, default=DEFAULT_WORLD)
    parser.add_argument("--db", type=Path, default=DEFAULT_DB)
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT)
    args = parser.parse_args()

    env = Environment(str(args.bundle), str(args.mono), str(args.world))
    con = sqlite3.connect(args.db)
    try:
        named_candidates = build_named_candidates(con)
        direct_items_by_folder: dict[str, dict[str, object]] = {}
        enchantments_by_folder: dict[str, list[dict[str, object]]] = {}

        for path, obj in env.container.items():
            m_item = ENTITY_ITEM_PAT.search(path) or ARMAMENT_ITEM_PAT.search(path)
            m_ench = ENCH_PAT.search(path)
            if not m_item and not m_ench:
                continue

            try:
                data = obj.read()
            except Exception:
                continue

            if m_item:
                folder = m_item.group(1)
                guid = asset_guid(data)
                if guid:
                    direct_items_by_folder[folder] = {
                        "guid": str(guid),
                        "name": getattr(data, "m_Name", folder) or folder,
                        "displayName": None,
                        "scriptType": script_type(data),
                        "path": path,
                    }

            if m_ench:
                folder, _ = m_ench.groups()
                name = getattr(data, "m_Name", None) or None
                if not name or name.endswith("ModifierA") or "StatusData" in name:
                    continue
                guid = asset_guid(data)
                if guid is None:
                    continue
                enchantments_by_folder.setdefault(folder, []).append(
                    {
                        "guid": str(guid),
                        "name": name,
                        "displayName": None,
                        "scriptType": script_type(data),
                        "path": path,
                    }
                )

        for folder, item in direct_items_by_folder.items():
            direct_items_by_folder[folder] = enrich_name(con, item)
        for folder, entries in enchantments_by_folder.items():
            enchantments_by_folder[folder] = [enrich_name(con, e) for e in entries]
        apply_enchantment_name_overrides(enchantments_by_folder)

        items = []
        seen_item_guids: set[str] = set()

        for folder, item in sorted(direct_items_by_folder.items()):
            enchs = sorted(enchantments_by_folder.get(folder, []), key=lambda e: str(e["name"]))
            if not enchs:
                continue
            items.append(
                {
                    "source": "direct_bundle_folder",
                    "bundleFolder": folder,
                    "item": item,
                    "enchantments": enchs,
                }
            )
            seen_item_guids.add(str(item["guid"]))

        for item_name, desired in sorted(named_candidates.items()):
            if not desired:
                continue

            matched_folder = None
            for folder, enchs in enchantments_by_folder.items():
                available = {str(e["name"]) for e in enchs}
                if desired.issubset(available):
                    matched_folder = folder
                    break
            if matched_folder is None:
                continue

            item = choose_base_item(con, item_name)
            if item is None or str(item["guid"]) in seen_item_guids:
                continue

            folder_entries = sorted(
                [e for e in enchantments_by_folder[matched_folder] if str(e["name"]) in desired],
                key=lambda e: str(e["name"]),
            )
            items.append(
                {
                    "source": "named_enchantment_bundle_match",
                    "bundleFolder": matched_folder,
                    "item": item,
                    "enchantments": folder_entries,
                }
            )
            seen_item_guids.add(str(item["guid"]))
    finally:
        con.close()

    items.sort(key=lambda row: (str(row["item"]["scriptType"] or ""), str(row["item"]["name"])))
    payload = {
        "items": items,
    }
    args.out.write_text(json.dumps(payload, indent=2) + "\n")
    print(f"Wrote {len(items)} mappings to {args.out}")


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""
Validate enchantment roll computation by merging save JSON with catalog DB.

Decodes Quality → normalized q, then interpolates against roll metadata
from the catalog to produce the "real text" shown in-game.

Rounding pipeline (from docs/enchantments.md):
  1. decode Quality → fixed (integer division)
  2. interpolate in FP16.16: min_fp + fp_mul(range_fp, fixed)
  3. ScaledModifier::Round(value, precision) — banker's rounding
  4. preview: (value * multiplier + 0x8000) >> 16 — round-half-up
  5. sign applied last for negative enchantments

Known limitations:
  - We don't know each modifier's rounding precision or preview multiplier
  - Using simple round-half-up as approximation; some values may be off by ±1

Usage:
    python3 scripts/validate_rolls.py [catalog.db] [save.json]
"""

import json
import math
import re
import sqlite3
import sys

QUALITY_DIVISOR = 0xFFFF00010000
QUALITY_FIXED_ONE = 65536

DB_PATH = sys.argv[1] if len(sys.argv) > 1 else "public/catalog.db"
JSON_PATH = sys.argv[2] if len(sys.argv) > 2 else "/tmp/char_save_filled.json"

RANGE_RE = re.compile(r'\(\s*[\d.]+%?\s*-\s*[\d.]+%?\s*\)\s*%?\s*')


def decode_quality(raw: int) -> tuple[int, float]:
    """Decode a u64 Quality into (fixed, normalized).
    fixed = integer result of Quality // DIVISOR (FP16.16 raw)
    normalized = fixed / 65536 (float 0..1)
    """
    if raw == 0:
        return 0, 0.0
    fixed = raw // QUALITY_DIVISOR
    normalized = fixed / QUALITY_FIXED_ONE
    return fixed, min(1.0, normalized)


def round_half_up(x: float) -> int:
    """Round to nearest integer, .5 goes up (matches game preview rounding)."""
    return math.floor(x + 0.5)


def compute_rolled_value(q: float, roll_kind: str, roll_min, roll_max,
                         roll_value, roll_unit: str, is_neg: bool):
    """Compute (rolled_float, rolled_int, unit) from quality + roll metadata."""
    if roll_kind == "special":
        return None, None, None
    if roll_kind == "fixed":
        v = roll_value
        return v, round_half_up(v) if v == int(v) else v, roll_unit
    if roll_kind == "range" and roll_min is not None and roll_max is not None:
        t = (1.0 - q) if is_neg else q
        rolled = roll_min + (roll_max - roll_min) * t
        return rolled, round_half_up(rolled), roll_unit
    return None, None, None


def format_display(val_int, unit: str) -> str:
    """Format integer + unit for display."""
    if val_int is None:
        return ""
    if isinstance(val_int, float) and val_int != int(val_int):
        s = f"{val_int:g}"
    else:
        s = str(int(val_int))
    return f"{s}{unit}" if unit == "%" else s


def make_real_text(effect_text: str, roll_kind: str, roll_min, roll_max,
                   roll_value, roll_unit: str, q: float, is_neg: bool) -> str:
    """Replace the range in effect_text with the computed rolled integer."""
    _, val_int, unit = compute_rolled_value(
        q, roll_kind, roll_min, roll_max, roll_value, roll_unit, is_neg)
    unit = unit or ""

    if roll_kind == "range" and val_int is not None:
        formatted = format_display(val_int, unit)
        return RANGE_RE.sub(formatted + " ", effect_text, count=1).rstrip()

    if roll_kind == "fixed":
        return effect_text

    return effect_text


def extract_item_name(item: dict) -> str:
    """Get the display name from an inventory item."""
    item_data = item.get("ItemData", {})
    if isinstance(item_data, dict):
        asset_id = item_data.get("Id", {})
        if isinstance(asset_id, dict) and asset_id.get("displayName"):
            return asset_id["displayName"]
    return "?"


def extract_guid_name(obj: dict) -> tuple[str | None, str]:
    """Extract ($guid, displayName) from an Asset.Id dict."""
    asset_id = obj.get("Asset", {}).get("Id", {})
    if isinstance(asset_id, dict):
        return asset_id.get("$guid"), asset_id.get("displayName", "?")
    return (str(asset_id) if asset_id else None), "?"


def collect_enchants(data: list) -> list[dict]:
    """Walk the save JSON and collect all enchantment/trait/gem instances."""
    results = []

    for doc_idx, doc in enumerate(data):
        content = doc.get("content", {})
        inventory = content.get("Inventory", [])

        for inv_idx, slot in enumerate(inventory):
            item = slot.get("Item", {})
            item_name = extract_item_name(item)

            ench_block = item.get("Enchantment", {})
            if not ench_block.get("HasValue"):
                continue

            value = ench_block.get("value", {})

            for ench in (value.get("Enchantment") or []):
                if not isinstance(ench, dict):
                    continue
                guid, name = extract_guid_name(ench)
                results.append({
                    "item": item_name, "type": "enchantment",
                    "guid": guid, "name": name,
                    "quality_raw": ench.get("Quality", 0),
                    "exalt": ench.get("ExaltStacks", 0),
                })

            for gem in (value.get("Gems") if isinstance(value.get("Gems"), list) else []):
                if not isinstance(gem, dict):
                    continue
                gem_ench = gem.get("Enchantment", {})
                if not isinstance(gem_ench, dict):
                    continue
                guid, name = extract_guid_name(gem_ench)
                if guid:
                    gem_ref = gem.get("GemRef", {}).get("Id", {})
                    gem_name = gem_ref.get("displayName", "?") if isinstance(gem_ref, dict) else "?"
                    results.append({
                        "item": item_name, "type": f"gem ({gem_name})",
                        "guid": guid, "name": name,
                        "quality_raw": gem_ench.get("Quality", 0),
                        "exalt": gem_ench.get("ExaltStacks", 0),
                    })

            trait = value.get("Trait", {})
            trait_guid, trait_name = extract_guid_name(trait)
            if trait_guid:
                results.append({
                    "item": item_name, "type": "trait",
                    "guid": trait_guid, "name": trait_name,
                    "quality_raw": trait.get("Quality", 0),
                    "exalt": trait.get("ExaltStacks", 0),
                })

    return results


def main():
    conn = sqlite3.connect(DB_PATH)
    conn.row_factory = sqlite3.Row

    data = json.load(open(JSON_PATH))
    enchants = collect_enchants(data)

    # Group by item
    by_item: dict[str, list] = {}
    for e in enchants:
        by_item.setdefault(e["item"], []).append(e)

    print(f"Found {len(enchants)} modifier instances across {len(by_item)} items\n")

    for item_name, entries in by_item.items():
        print(f"=== {item_name} ===")

        for e in entries:
            guid = e["guid"]
            if not guid:
                continue

            _fixed, q = decode_quality(e["quality_raw"])

            row = conn.execute(
                "SELECT effect_text, roll_kind, roll_min, roll_max, roll_value, roll_unit, roll_is_negative "
                "FROM modifier_details WHERE asset_guid = ?",
                (int(guid),)
            ).fetchone()

            label = e["type"]
            q_pct = f"q={q * 100:.0f}%"

            if row:
                is_neg = bool(row["roll_is_negative"])
                real_text = make_real_text(
                    row["effect_text"], row["roll_kind"],
                    row["roll_min"], row["roll_max"],
                    row["roll_value"], row["roll_unit"],
                    q, is_neg,
                )
                neg = " [NEG]" if is_neg else ""

                # Show raw interpolated value for ranges to help spot rounding issues
                detail = ""
                if row["roll_kind"] == "range" and row["roll_min"] is not None:
                    t = (1.0 - q) if is_neg else q
                    raw_val = row["roll_min"] + (row["roll_max"] - row["roll_min"]) * t
                    unit = row["roll_unit"] or ""
                    detail = f"  (raw={raw_val:.2f}{unit}, {q_pct})"

                print(f"  [{label}] {real_text}{detail}{neg}")
            else:
                print(f"  [{label}] {e['name']}  ({q_pct}, not in catalog)")

        print()

    conn.close()


if __name__ == "__main__":
    main()

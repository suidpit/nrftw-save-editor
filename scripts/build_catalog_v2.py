# /// script
# requires-python = ">=3.12"
# dependencies = [
#     "beautifulsoup4",
#     "requests",
# ]
# ///
"""
Build catalog 2.0 from the raw bundle catalog and site enrichment.

The builder:
- scopes work to inventory-editing asset types plus EnchantmentDataAsset
- archives fetched site pages to disk for offline reparsing
- rate-limits and retries requests with jittered backoff
- keeps a compatibility `assets` table for current app/runtime code
- writes normalized detail and audit tables for future UI use

Usage:
    uv run --script scripts/build_catalog_v2.py --input-db public/catalog.raw.db --output-db public/catalog.db
    uv run --script scripts/build_catalog_v2.py --offline --input-db public/catalog.raw.db --output-db public/catalog.db
"""

from __future__ import annotations

import argparse
import concurrent.futures
import hashlib
import json
import random
import re
import shutil
import sqlite3
import subprocess
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any
from urllib.parse import urljoin, urlparse

import requests
from bs4 import BeautifulSoup

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_INPUT_DB = ROOT / "public" / "catalog.raw.db"
DEFAULT_OUTPUT_DB = ROOT / "public" / "catalog.db"
DEFAULT_CACHE_DIR = ROOT / "cache" / "catalog-v2"
SITE_ROOT = "https://www.norestforthewicked.gg"
DB_ROOT_PATH = "/db"
REQUEST_TIMEOUT = 30
DEFAULT_MAX_WORKERS = 10
DEFAULT_MIN_DELAY = 1.0
DEFAULT_MAX_DELAY = 3.0
MAX_RETRIES = 4

INDEX_PAGE_URLS = (
    "https://www.norestforthewicked.gg/db/weapons",
    "https://www.norestforthewicked.gg/db/quivers/quiver",
    "https://www.norestforthewicked.gg/db/shields",
    "https://www.norestforthewicked.gg/db/trinkets/ring",
    "https://www.norestforthewicked.gg/db/armor",
    "https://www.norestforthewicked.gg/db/runes",
    "https://www.norestforthewicked.gg/db/gems",
    "https://www.norestforthewicked.gg/db/food",
    "https://www.norestforthewicked.gg/db/quick",
    "https://www.norestforthewicked.gg/db/throwable",
    "https://www.norestforthewicked.gg/db/ingredients",
    "https://www.norestforthewicked.gg/db/resources",
    "https://www.norestforthewicked.gg/db/refined",
    "https://www.norestforthewicked.gg/db/components",
    "https://www.norestforthewicked.gg/db/fuels",
    "https://www.norestforthewicked.gg/db/embers",
    "https://www.norestforthewicked.gg/db/keys",
    "https://www.norestforthewicked.gg/db/quest-items",
    "https://www.norestforthewicked.gg/db/other",
    "https://www.norestforthewicked.gg/db/tools",
    "https://www.norestforthewicked.gg/db/house",
    "https://www.norestforthewicked.gg/db/enchants",
)

COMPATIBILITY_LABEL_HINTS = (
    "compatible",
    "only on",
    "works on",
    "works with",
    "socket",
    "equipment",
    "applies to",
    "use on",
    "slot",
)

VALUE_LABEL_HINTS = {
    "source",
    "sell value",
    "value",
    "weight",
    "durability",
    "focus cost",
    "focus gain",
    "requirements",
    "damage",
    "damage type",
    "scaling",
}


def normalize_text(value: str) -> str:
    return re.sub(r"[^a-z0-9]+", "", value.lower())


def normalize_whitespace(value: str) -> str:
    return " ".join(value.split()).strip()


def sanitize_slug(value: str) -> str:
    slug = re.sub(r"[^a-zA-Z0-9._-]+", "-", value.strip()).strip("-")
    return slug or "page"


def sha256_text(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def load_inventory_asset_types(ts_path: Path) -> list[str]:
    text = ts_path.read_text(encoding="utf-8")
    return re.findall(r'value:\s*"([^"]+)"', text)


def classify_target(asset_type: str, display_name: str = "", site_path: str = "") -> tuple[str | None, str | None]:
    normalized_name = normalize_text(display_name)
    path = site_path.lower()

    if asset_type == "WeaponStaticDataAsset":
        if "wand" in normalized_name or "/wand" in path or "/wands/" in path:
            return "weapon", "wand"
        if "shield" in normalized_name or "/shield" in path or "/shields/" in path:
            return "weapon", "shield"
        if "bow" in normalized_name or "/bow" in path or "/bows/" in path:
            return "weapon", "bow"
        if "spear" in normalized_name or "/spear" in path or "/spears/" in path:
            return "weapon", "spear"
        if "staff" in normalized_name or "/staff" in path or "/staves/" in path:
            return "weapon", "staff"
        if "axe" in normalized_name or "/axe" in path or "/axes/" in path:
            return "weapon", "axe"
        if "dagger" in normalized_name or "/dagger" in path or "/daggers/" in path:
            return "weapon", "dagger"
        if "sword" in normalized_name or "/sword" in path or "/swords/" in path:
            return "weapon", "sword"
        return "weapon", None
    if asset_type == "RingsDataAsset":
        return "ring", None
    if asset_type == "HelmDataAsset":
        return "helm", None
    if asset_type == "BodyDataAsset":
        return "body", None
    if asset_type == "PantsDataAsset":
        return "pants", None
    if asset_type == "GlovesDataAsset":
        return "gloves", None
    return None, None


def modifier_kind_for_asset_type(asset_type: str, site_category: str) -> str | None:
    if asset_type == "EnchantmentDataAsset":
        return "enchantment"
    if asset_type == "EnchantGemItemDataAsset":
        return "gem"
    if asset_type == "HeroRuneDataAsset":
        return "rune"
    if site_category == "runes":
        return "rune"
    return None


def category_candidates_for_asset_type(asset_type: str) -> set[str]:
    if asset_type == "WeaponStaticDataAsset":
        return {"weapons"}
    if asset_type in {"HelmDataAsset", "BodyDataAsset", "PantsDataAsset", "GlovesDataAsset"}:
        return {"armor"}
    if asset_type == "RingsDataAsset":
        return {"trinkets"}
    if asset_type in {
        "GenericItemDataAsset",
        "QuickItemDataAsset",
        "FoodItemDataAsset",
        "GenericToolItemDataAsset",
        "ThrowableItemDataAsset",
        "EquipmentManipulationItemDataAsset",
        "FuelItemDataAsset",
    }:
        return {"items"}
    if asset_type == "HeroRuneDataAsset":
        return {"runes"}
    if asset_type == "EnchantGemItemDataAsset":
        return {"gems"}
    if asset_type == "EnchantmentDataAsset":
        return {"enchants"}
    return set()


@dataclass(slots=True)
class LocalAsset:
    asset_guid: str
    name: str
    display_name: str
    bundle: str | None
    data: str | None
    script_type: str
    icon_png: bytes | None
    target_kind: str | None
    target_subkind: str | None


@dataclass(slots=True)
class SiteEntity:
    path: str
    guid: str | None
    category: str
    subcategory: str | None
    title: str
    source_url: str


@dataclass(slots=True)
class MatchResult:
    asset_guid: str
    status: str
    site_path: str | None
    site_guid: str | None
    reason: str


@dataclass(slots=True)
class EnchantTableRow:
    description: str
    description_prefix: str
    type_name: str
    group_name: str
    drop_level: str
    drop_rate: str
    affects_stat: str
    applies_to: str
    only_on_items: str


class AdaptiveLimiter:
    def __init__(self, max_parallelism: int, min_delay: float, max_delay: float):
        self.base_parallelism = max_parallelism
        self.max_parallelism = max_parallelism
        self.min_delay = min_delay
        self.max_delay = max_delay
        self.delay_multiplier = 1.0
        self.active_requests = 0
        self._lock = threading.Condition()

    def __enter__(self) -> "AdaptiveLimiter":
        with self._lock:
            while self.active_requests >= self.max_parallelism:
                self._lock.wait()
            self.active_requests += 1
            delay = random.uniform(self.min_delay, self.max_delay) * self.delay_multiplier
        time.sleep(delay)
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        with self._lock:
            self.active_requests -= 1
            self._lock.notify_all()

    def record_success(self, elapsed: float) -> None:
        with self._lock:
            if elapsed > 8.0:
                self.delay_multiplier = min(self.delay_multiplier * 1.15, 8.0)
            else:
                self.delay_multiplier = max(1.0, self.delay_multiplier * 0.97)
                if self.delay_multiplier <= 1.25:
                    self.max_parallelism = min(self.base_parallelism, self.max_parallelism + 1)

    def record_backpressure(self) -> None:
        with self._lock:
            self.delay_multiplier = min(self.delay_multiplier * 1.75, 10.0)
            self.max_parallelism = max(1, self.max_parallelism - 1)


class PageArchive:
    def __init__(self, base_dir: Path):
        self.base_dir = base_dir
        self.pages_dir = base_dir / "site-pages"
        self.index_dir = base_dir / "site-index"
        self.meta_dir = base_dir / "site-meta"
        self.manifest_path = self.meta_dir / "manifest.json"
        self.pages_dir.mkdir(parents=True, exist_ok=True)
        self.index_dir.mkdir(parents=True, exist_ok=True)
        self.meta_dir.mkdir(parents=True, exist_ok=True)
        self._lock = threading.Lock()
        if self.manifest_path.exists():
            self.manifest = json.loads(self.manifest_path.read_text(encoding="utf-8"))
        else:
            self.manifest = {"pages": {}}

    def _persist(self) -> None:
        self.manifest_path.write_text(
            json.dumps(self.manifest, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )

    def cache_path_for(self, url: str, guid: str | None, is_index: bool) -> Path:
        parsed = urlparse(url)
        tail = parsed.path.rstrip("/").split("/")[-1] or "db"
        if guid:
            filename = f"{guid}.html"
        else:
            filename = f"{sanitize_slug(tail)}-{sha256_text(url)[:10]}.html"
        directory = self.index_dir if is_index else self.pages_dir
        return directory / filename

    def load(self, url: str) -> str | None:
        html, _status = self.load_with_status(url)
        return html

    def load_with_status(self, url: str) -> tuple[str | None, int | None]:
        with self._lock:
            record = self.manifest["pages"].get(url)
        if not record:
            return None, None
        path = Path(record["path"])
        if not path.exists():
            return None, record.get("status")
        return path.read_text(encoding="utf-8"), record.get("status")

    def save(
        self,
        *,
        url: str,
        guid: str | None,
        is_index: bool,
        html: str,
        status: int,
        category: str | None,
        subcategory: str | None,
        title: str | None,
    ) -> Path:
        path = self.cache_path_for(url, guid, is_index)
        path.write_text(html, encoding="utf-8")
        record = {
            "path": str(path),
            "guid": guid,
            "is_index": is_index,
            "status": status,
            "sha256": sha256_text(html),
            "fetched_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "category": category,
            "subcategory": subcategory,
            "title": title,
        }
        with self._lock:
            self.manifest["pages"][url] = record
            self._persist()
        return path


class SiteFetcher:
    def __init__(
        self,
        archive: PageArchive,
        *,
        offline: bool,
        max_workers: int,
        min_delay: float,
        max_delay: float,
        user_agent: str,
    ):
        self.archive = archive
        self.offline = offline
        self.session = requests.Session()
        self.session.headers.update({"User-Agent": user_agent})
        self.limiter = AdaptiveLimiter(max_workers, min_delay, max_delay)

    def close(self) -> None:
        self.session.close()

    def fetch(self, url: str, *, guid: str | None = None, is_index: bool = False) -> str:
        cached = self.archive.load(url)
        if cached is not None:
            return cached
        if self.offline:
            raise RuntimeError(f"offline mode: missing cached page for {url}")

        last_error: Exception | None = None
        for attempt in range(1, MAX_RETRIES + 1):
            started = time.monotonic()
            try:
                with self.limiter:
                    response = self.session.get(url, timeout=REQUEST_TIMEOUT)
                elapsed = time.monotonic() - started
                if response.status_code in {403, 429}:
                    self.limiter.record_backpressure()
                    raise RuntimeError(f"{response.status_code} from {url}")
                response.raise_for_status()
                self.limiter.record_success(elapsed)
                html = response.text
                soup = BeautifulSoup(html, "html.parser")
                title = soup.title.get_text(" ", strip=True) if soup.title else None
                category, subcategory = category_from_path(urlparse(url).path)
                self.archive.save(
                    url=url,
                    guid=guid,
                    is_index=is_index,
                    html=html,
                    status=response.status_code,
                    category=category,
                    subcategory=subcategory,
                    title=title,
                )
                return html
            except Exception as exc:  # noqa: BLE001
                last_error = exc
                if attempt >= MAX_RETRIES:
                    break
                self.limiter.record_backpressure()
                sleep_for = min(45.0, (2 ** attempt) + random.uniform(0.5, 2.0))
                time.sleep(sleep_for)
        raise RuntimeError(f"failed to fetch {url}: {last_error}") from last_error

    def fetch_optional(self, url: str, *, guid: str | None = None, is_index: bool = False) -> tuple[str | None, int | None]:
        cached, cached_status = self.archive.load_with_status(url)
        if cached_status == 404:
            return None, 404
        if cached is not None:
            return cached, cached_status
        if self.offline:
            return None, None

        last_error: Exception | None = None
        for attempt in range(1, MAX_RETRIES + 1):
            started = time.monotonic()
            try:
                with self.limiter:
                    response = self.session.get(url, timeout=REQUEST_TIMEOUT)
                elapsed = time.monotonic() - started
                if response.status_code in {403, 429}:
                    self.limiter.record_backpressure()
                    raise RuntimeError(f"{response.status_code} from {url}")
                if response.status_code == 404:
                    html = response.text
                    soup = BeautifulSoup(html, "html.parser")
                    title = soup.title.get_text(" ", strip=True) if soup.title else None
                    category, subcategory = category_from_path(urlparse(url).path)
                    self.archive.save(
                        url=url,
                        guid=guid,
                        is_index=is_index,
                        html=html,
                        status=response.status_code,
                        category=category,
                        subcategory=subcategory,
                        title=title,
                    )
                    self.limiter.record_success(elapsed)
                    return None, 404
                response.raise_for_status()
                self.limiter.record_success(elapsed)
                html = response.text
                soup = BeautifulSoup(html, "html.parser")
                title = soup.title.get_text(" ", strip=True) if soup.title else None
                category, subcategory = category_from_path(urlparse(url).path)
                self.archive.save(
                    url=url,
                    guid=guid,
                    is_index=is_index,
                    html=html,
                    status=response.status_code,
                    category=category,
                    subcategory=subcategory,
                    title=title,
                )
                return html, response.status_code
            except Exception as exc:  # noqa: BLE001
                last_error = exc
                if attempt >= MAX_RETRIES:
                    break
                self.limiter.record_backpressure()
                sleep_for = min(45.0, (2 ** attempt) + random.uniform(0.5, 2.0))
                time.sleep(sleep_for)
        raise RuntimeError(f"failed to fetch {url}: {last_error}") from last_error


def category_from_path(path: str) -> tuple[str | None, str | None]:
    parts = [part for part in path.split("/") if part]
    if len(parts) < 2 or parts[0] != "db":
        return None, None
    category = parts[1]
    subcategory = parts[2] if len(parts) > 3 else None
    return category, subcategory


def extract_site_guid(path: str) -> str | None:
    match = re.search(r"/(\d+)(?:/)?$", path)
    return match.group(1) if match else None


def extract_site_entities(index_url: str, html: str) -> list[SiteEntity]:
    soup = BeautifulSoup(html, "html.parser")
    entities: dict[str, SiteEntity] = {}
    source_parsed = urlparse(index_url)
    source_category, source_subcategory = category_from_path(source_parsed.path)
    for link in soup.find_all("a", href=True):
        parsed = urlparse(urljoin(SITE_ROOT, link["href"]))
        if parsed.netloc and parsed.netloc != urlparse(SITE_ROOT).netloc:
            continue
        path = parsed.path.rstrip("/")
        parts = [part for part in path.split("/") if part]
        if len(parts) < 3 or parts[0] != "db":
            continue
        guid = extract_site_guid(path)
        if guid is None:
            continue
        title = " ".join(link.stripped_strings).strip()
        if not title and guid is None:
            continue
        category = parts[1]
        subcategory = parts[2] if len(parts) > 3 else source_subcategory
        entities[path] = SiteEntity(
            path=path,
            guid=guid,
            category=category,
            subcategory=subcategory,
            title=title or guid or path.split("/")[-1],
            source_url=index_url,
        )
    return list(entities.values())


def text_lines_from_soup(soup: BeautifulSoup) -> list[str]:
    body = soup.body or soup
    text = body.get_text("\n", strip=True)
    return [line.strip() for line in text.splitlines() if line.strip()]


def structured_fields_from_soup(soup: BeautifulSoup) -> dict[str, str]:
    fields: dict[str, str] = {}

    for row in soup.select("table tr"):
        cells = row.find_all(["th", "td"])
        if len(cells) < 2:
            continue
        key = cells[0].get_text(" ", strip=True)
        value = cells[1].get_text(" ", strip=True)
        if key and value and key not in fields:
            fields[key] = value

    for dl in soup.find_all("dl"):
        terms = dl.find_all("dt")
        descriptions = dl.find_all("dd")
        for term, desc in zip(terms, descriptions):
            key = term.get_text(" ", strip=True)
            value = desc.get_text(" ", strip=True)
            if key and value and key not in fields:
                fields[key] = value

    headings = soup.find_all(re.compile(r"^h[1-6]$"))
    for heading in headings:
        heading_text = heading.get_text(" ", strip=True)
        next_list = heading.find_next(["ul", "ol"])
        if not heading_text or next_list is None:
            continue
        if any(hint in heading_text.lower() for hint in COMPATIBILITY_LABEL_HINTS):
            values = [li.get_text(" ", strip=True) for li in next_list.find_all("li")]
            values = [value for value in values if value]
            if values and heading_text not in fields:
                fields[heading_text] = ", ".join(values)

    return fields


def extract_effect_text(lines: list[str], fields: dict[str, str]) -> str | None:
    for key, value in fields.items():
        if any(hint in key.lower() for hint in ("effect", "bonus", "description")):
            return value
    for line in lines:
        if 10 <= len(line) <= 240 and any(ch.isdigit() for ch in line):
            return line
    for line in lines:
        if 10 <= len(line) <= 240:
            return line
    return None


def extract_labeled_row_value(item_card: BeautifulSoup | None, label: str) -> str | None:
    if item_card is None:
        return None
    normalized_label = normalize_whitespace(label).casefold()
    for row in item_card.find_all("div", recursive=False):
        children = row.find_all("div", recursive=False)
        if len(children) < 2:
            continue
        first_text = normalize_whitespace(children[0].get_text(" ", strip=True)).rstrip(":")
        if first_text.casefold() != normalized_label:
            continue
        value = normalize_whitespace(" ".join(child.get_text(" ", strip=True) for child in children[1:]))
        return value or None
    return None


def extract_item_card_enchant_text(soup: BeautifulSoup) -> str | None:
    node = soup.select_one(".item-card .enchant-text")
    if node is None:
        return None
    text = normalize_whitespace(node.get_text(" ", strip=True))
    return text or None


def extract_equipment_slot_counts(soup: BeautifulSoup) -> tuple[int | None, int | None]:
    item_card = soup.select_one("div.item-card")
    if item_card is None:
        return None, None

    rune_slots: int | None = None
    rune_grid = item_card.select_one("div.item-rune-grid")
    if rune_grid is not None:
        rune_slots = len(
            rune_grid.find_all(
                ["a", "div"],
                class_=lambda value: isinstance(value, list) and ("item-rune" in value or "item-link" in value),
                recursive=False,
            )
        )
        if rune_slots == 0:
            rune_slots = None

    gem_slots: int | None = None
    for gem_node in item_card.select("div.item-gem"):
        gem_text = normalize_whitespace(gem_node.get_text(" ", strip=True))
        multiplier_match = re.search(r"[×x]\s*(\d+)", gem_text)
        if multiplier_match:
            gem_slots = int(multiplier_match.group(1))
            break
    if gem_slots is None:
        for gem_text in soup.find_all(string=re.compile(r"Empty\s*[×x]\s*\d+")):
            multiplier_match = re.search(r"[×x]\s*(\d+)", str(gem_text))
            if multiplier_match:
                gem_slots = int(multiplier_match.group(1))
                break

    return rune_slots, gem_slots


def normalize_handling(text: str) -> str | None:
    lowered = text.lower()
    if re.search(r"(?<![a-z])dual[- ]wield(?:ing)?(?![a-z])", lowered):
        return "dual_wield"
    if re.search(r"(?<![a-z])one[- ]handed(?![a-z])", lowered):
        return "one_handed"
    if re.search(r"(?<![a-z])two[- ]handed(?![a-z])", lowered):
        return "two_handed"
    return None


def normalize_target_tokens(text: str) -> list[tuple[str, str | None]]:
    lowered = text.lower()
    pairs: list[tuple[str, str | None]] = []

    direct_map = (
        ("utility slot", ("utility", None)),
        ("utility", ("utility", None)),
        ("all armor", ("helm", None)),
        ("all armor", ("body", None)),
        ("all armor", ("pants", None)),
        ("all armor", ("gloves", None)),
        ("body armor", ("body", None)),
        ("body", ("body", None)),
        ("one-handed weapon", ("weapon", None)),
        ("two-handed weapon", ("weapon", None)),
        ("one handed weapon", ("weapon", None)),
        ("two handed weapon", ("weapon", None)),
        ("gauntlets", ("gloves", None)),
        ("gloves", ("gloves", None)),
        ("trousers", ("pants", None)),
        ("pants", ("pants", None)),
        ("helmets", ("helm", None)),
        ("helmet", ("helm", None)),
        ("shields", ("weapon", "shield")),
        ("shield", ("weapon", "shield")),
        ("wands", ("weapon", "wand")),
        ("wand", ("weapon", "wand")),
        ("bows", ("weapon", "bow")),
        ("bow", ("weapon", "bow")),
        ("spears", ("weapon", "spear")),
        ("spear", ("weapon", "spear")),
        ("staves", ("weapon", "staff")),
        ("staff", ("weapon", "staff")),
        ("axes", ("weapon", "axe")),
        ("axe", ("weapon", "axe")),
        ("daggers", ("weapon", "dagger")),
        ("dagger", ("weapon", "dagger")),
        ("swords", ("weapon", "sword")),
        ("sword", ("weapon", "sword")),
        ("weapons", ("weapon", None)),
        ("weapon", ("weapon", None)),
        ("rings", ("ring", None)),
        ("ring", ("ring", None)),
        ("armor", ("body", None)),
        ("chest", ("body", None)),
    )

    for token, pair in direct_map:
        pattern = r"(?<![a-z])" + re.escape(token) + r"(?![a-z])"
        if re.search(pattern, lowered):
            pairs.append(pair)

    deduped: list[tuple[str, str | None]] = []
    seen: set[tuple[str, str | None]] = set()
    for pair in pairs:
        if pair not in seen:
            deduped.append(pair)
            seen.add(pair)
    return deduped


def extract_rendered_text_snapshot(lines: list[str], limit: int = 160) -> list[str]:
    return lines[:limit]


def default_gem_compatibility_rows(effect_text: str | None = None) -> list[dict[str, str | None]]:
    rows: list[dict[str, str | None]] = []
    for target_kind in ("weapon", "helm", "body", "pants", "gloves"):
        rows.append(
            {
                "target_kind": target_kind,
                "target_subkind": None,
                "source_label": "game_rule_default",
                "effect_text": effect_text,
                "required_handling": None,
            }
        )
    return rows


def extract_equipment_handling(entity: SiteEntity, title: str, fields: dict[str, str], lines: list[str]) -> str | None:
    if entity.category not in {"weapons", "shields"}:
        return None

    candidates: list[str] = [title]
    candidates.extend(value for value in fields.values() if isinstance(value, str))
    candidates.extend(lines[:200])
    for candidate in candidates:
        handling = normalize_handling(candidate)
        if handling is not None:
            return handling
    return None


def extract_sentence_compatibility(lines: list[str]) -> list[tuple[str, str | None, str, str | None]]:
    pairs: list[tuple[str, str | None, str, str | None, str | None]] = []
    for idx, line in enumerate(lines):
        lowered = line.lower()
        if "slot this rune into" in lowered:
            window = " ".join(lines[idx:min(len(lines), idx + 6)])
            required_handling = normalize_handling(window)
            for target_kind, target_subkind in normalize_target_tokens(window):
                pairs.append((target_kind, target_subkind, "slot_this_rune_into", window, required_handling))
        elif "slot this gem into" in lowered or "slot this enchantment into" in lowered:
            window = " ".join(lines[idx:min(len(lines), idx + 6)])
            required_handling = normalize_handling(window)
            for target_kind, target_subkind in normalize_target_tokens(window):
                pairs.append((target_kind, target_subkind, "slot_modifier_into", window, required_handling))
    deduped: list[tuple[str, str | None, str, str | None, str | None]] = []
    seen: set[tuple[str, str | None, str, str | None, str | None]] = set()
    for pair in pairs:
        if pair not in seen:
            deduped.append(pair)
            seen.add(pair)
    return deduped


def append_linked_modifier(
    entries: list[dict[str, Any]],
    seen_keys: set[tuple[str | None, str | None, str]],
    *,
    section: str,
    path: str | None,
    guid: str | None,
    title: str,
    category: str | None,
    subcategory: str | None,
) -> None:
    normalized_title = " ".join(title.split()).strip()
    if not normalized_title:
        return
    key = (path, guid, normalized_title.casefold())
    if key in seen_keys:
        return
    entries.append(
        {
            "section": section,
            "path": path,
            "guid": guid,
            "title": normalized_title,
            "category": category,
            "subcategory": subcategory,
        }
    )
    seen_keys.add(key)


def extract_item_card_modifiers(soup: BeautifulSoup) -> list[dict[str, Any]]:
    item_card = soup.select_one("div.item-card")
    if item_card is None:
        return []

    entries: list[dict[str, Any]] = []
    seen_keys: set[tuple[str | None, str | None, str]] = set()
    content = item_card.select_one("div.content")
    if content is None:
        return entries

    content_children = [child for child in content.children if getattr(child, "name", None)]

    rune_grid = content.select_one("div.item-rune-grid")
    if rune_grid is not None:
        for link in rune_grid.find_all("a", href=True):
            parsed = urlparse(urljoin(SITE_ROOT, link["href"]))
            linked_path = parsed.path.rstrip("/")
            linked_guid = extract_site_guid(linked_path)
            linked_title = link.get_text(" ", strip=True)
            linked_category, linked_subcategory = category_from_path(linked_path)
            append_linked_modifier(
                entries,
                seen_keys,
                section="Runes",
                path=linked_path,
                guid=linked_guid,
                title=linked_title,
                category=linked_category,
                subcategory=linked_subcategory,
            )

    gems_index = next(
        (idx for idx, child in enumerate(content_children) if "Gems" in child.get_text(" ", strip=True).split()),
        None,
    )
    if gems_index is not None:
        for child in content_children[gems_index + 1:]:
            child_classes = set(child.get("class", []))
            if "enchant-panel" in child_classes or "item-description" in child_classes:
                break
            for link in child.find_all("a", href=True):
                parsed = urlparse(urljoin(SITE_ROOT, link["href"]))
                linked_path = parsed.path.rstrip("/")
                linked_guid = extract_site_guid(linked_path)
                linked_title = link.get_text(" ", strip=True)
                linked_category, linked_subcategory = category_from_path(linked_path)
                append_linked_modifier(
                    entries,
                    seen_keys,
                    section="Gems",
                    path=linked_path,
                    guid=linked_guid,
                    title=linked_title,
                    category=linked_category,
                    subcategory=linked_subcategory,
                )

    enchant_panel = content.select_one("div.enchant-panel")
    if enchant_panel is not None:
        for enchant in enchant_panel.select("div.enchant"):
            linked_entries = 0
            for link in enchant.find_all("a", href=True):
                parsed = urlparse(urljoin(SITE_ROOT, link["href"]))
                linked_path = parsed.path.rstrip("/")
                linked_guid = extract_site_guid(linked_path)
                linked_title = link.get_text(" ", strip=True)
                linked_category, linked_subcategory = category_from_path(linked_path)
                append_linked_modifier(
                    entries,
                    seen_keys,
                    section="Enchantments",
                    path=linked_path,
                    guid=linked_guid,
                    title=linked_title,
                    category=linked_category,
                    subcategory=linked_subcategory,
                )
                linked_entries += 1
            if linked_entries == 0:
                title = enchant.get_text(" ", strip=True)
                append_linked_modifier(
                    entries,
                    seen_keys,
                    section="Enchantments",
                    path=None,
                    guid=None,
                    title=title,
                    category="enchants",
                    subcategory=None,
                )

    return entries


def infer_modifier_type_hint(html: str) -> str | None:
    lowered = html.lower()
    if "plagued" in lowered:
        return "Plagued"
    if "magical" in lowered:
        return "Magical"
    return None


def detect_preexisting_modifier_sections(soup: BeautifulSoup) -> dict[str, bool]:
    item_card = soup.select_one("div.item-card")
    if item_card is None:
        return {
            "runes": False,
            "gems": False,
            "enchantments": False,
        }

    content = item_card.select_one("div.content")
    if content is None:
        return {
            "runes": False,
            "gems": False,
            "enchantments": False,
        }

    content_text = content.get_text(" ", strip=True)
    return {
        "runes": content.select_one("div.item-rune-grid") is not None,
        "gems": "Gems" in content_text,
        "enchantments": content.select_one("div.enchant-panel") is not None or "Enchantments" in content_text,
    }


def parse_detail_page(entity: SiteEntity, html: str) -> dict[str, Any]:
    soup = BeautifulSoup(html, "html.parser")
    item_card = soup.select_one("div.item-card")
    title_tag = soup.find(re.compile(r"^h1$"))
    title = title_tag.get_text(" ", strip=True) if title_tag else entity.title
    fields = structured_fields_from_soup(soup)
    lines = text_lines_from_soup(soup)
    rendered_text = extract_rendered_text_snapshot(lines)
    warnings: list[str] = []
    enchant_display_text = extract_item_card_enchant_text(soup)
    detail_applies_to = extract_labeled_row_value(item_card, "Applies To")
    rune_slots, gem_slots = extract_equipment_slot_counts(soup)
    modifier_type_hint = infer_modifier_type_hint(html)

    compatibility_pairs: list[tuple[str, str | None, str, str | None, str | None]] = []
    for key, value in fields.items():
        if any(hint in key.lower() for hint in COMPATIBILITY_LABEL_HINTS):
            required_handling = normalize_handling(f"{key} {value}")
            for target_kind, target_subkind in normalize_target_tokens(value):
                compatibility_pairs.append((target_kind, target_subkind, key, value, required_handling))

    if not compatibility_pairs:
        for line in lines:
            lowered = line.lower()
            if any(hint in lowered for hint in COMPATIBILITY_LABEL_HINTS):
                required_handling = normalize_handling(line)
                for target_kind, target_subkind in normalize_target_tokens(line):
                    compatibility_pairs.append((target_kind, target_subkind, line, None, required_handling))

    if entity.category in {"runes", "gems", "enchants"}:
        sentence_pairs = extract_sentence_compatibility(lines)
        for pair in sentence_pairs:
            if pair not in compatibility_pairs:
                compatibility_pairs.append(pair)

    extracted_values: dict[str, str] = {}
    for key, value in fields.items():
        if key.lower() in VALUE_LABEL_HINTS or any(hint in key.lower() for hint in VALUE_LABEL_HINTS):
            extracted_values[key] = value

    linked_modifiers: list[dict[str, Any]] = extract_item_card_modifiers(soup)
    modifier_sections = detect_preexisting_modifier_sections(soup)
    seen_linked_keys = {
        (entry.get("path"), entry.get("guid"), str(entry.get("title", "")).casefold())
        for entry in linked_modifiers
    }
    page_title_link = soup.find("a", href=lambda href: isinstance(href, str) and href.rstrip("/") == entity.path)
    page_card = page_title_link
    if page_card is not None:
        for _ in range(6):
            if page_card is None:
                break
            if getattr(page_card, "find_all", None):
                candidate_links = page_card.find_all("a", href=True)
                if len(candidate_links) > 1:
                    break
            page_card = page_card.parent
        if page_card is not None:
            for link in page_card.find_all("a", href=True):
                parsed = urlparse(urljoin(SITE_ROOT, link["href"]))
                linked_path = parsed.path.rstrip("/")
                if linked_path == entity.path:
                    continue
                linked_guid = extract_site_guid(linked_path)
                linked_title = link.get_text(" ", strip=True)
                if not linked_title and linked_guid is None:
                    continue
                linked_category, linked_subcategory = category_from_path(linked_path)
                if linked_category not in {"enchants", "runes", "gems"}:
                    continue
                append_linked_modifier(
                    linked_modifiers,
                    seen_linked_keys,
                    section="item_card",
                    path=linked_path,
                    guid=linked_guid,
                    title=linked_title,
                    category=linked_category,
                    subcategory=linked_subcategory,
                )

    seen_linked_paths = {entry["path"] for entry in linked_modifiers if entry.get("path")}
    for heading in soup.find_all(re.compile(r"^h[1-6]$")):
        section_name = heading.get_text(" ", strip=True)
        lowered = section_name.lower()
        if not any(token in lowered for token in ("enchantment", "rune", "gem")):
            continue
        next_block = heading.find_next(["ul", "ol", "table", "div"])
        if next_block is None:
            continue
        for link in next_block.find_all("a", href=True):
            parsed = urlparse(urljoin(SITE_ROOT, link["href"]))
            linked_path = parsed.path.rstrip("/")
            if linked_path in seen_linked_paths:
                continue
            linked_guid = extract_site_guid(linked_path)
            linked_title = link.get_text(" ", strip=True)
            if not linked_title and linked_guid is None:
                continue
            linked_category, linked_subcategory = category_from_path(linked_path)
            if linked_category not in {"enchants", "runes", "gems"}:
                continue
            append_linked_modifier(
                linked_modifiers,
                seen_linked_keys,
                section=section_name,
                path=linked_path,
                guid=linked_guid,
                title=linked_title,
                category=linked_category,
                subcategory=linked_subcategory,
            )
            seen_linked_paths.add(linked_path)

    if entity.category == "runes" and not compatibility_pairs:
        warnings.append("compatibility_not_parsed")
    linked_modifier_categories = {
        str(entry.get("category") or "").lower()
        for entry in linked_modifiers
        if entry.get("category")
    }
    if entity.category in {"weapons", "rings"} and not linked_modifiers:
        if (
            entity.category == "weapons"
            and modifier_sections["enchantments"] is False
            and (rune_slots in {None, 0})
            and (gem_slots in {None, 0})
        ):
            pass
        else:
            warnings.append("preexisting_modifiers_not_found")
    if (
        entity.category == "shields"
        and modifier_sections["enchantments"]
        and "enchants" not in linked_modifier_categories
    ):
        warnings.append("preexisting_modifiers_not_found")

    return {
        "title": title,
        "category": entity.category,
        "subcategory": entity.subcategory,
        "guid": entity.guid,
        "path": entity.path,
        "source_url": entity.source_url,
        "handling": extract_equipment_handling(entity, title, fields, lines),
        "detail_applies_to": detail_applies_to,
        "modifier_type_hint": modifier_type_hint,
        "rune_slots": rune_slots,
        "gem_slots": gem_slots,
        "enchant_display_text": enchant_display_text,
        "fields": fields,
        "text_lines": rendered_text,
        "rendered_text": "\n".join(rendered_text),
        "warnings": warnings,
        "effect_text": enchant_display_text or extract_effect_text(lines, fields),
        "compatibility": [
            {
                "target_kind": target_kind,
                "target_subkind": target_subkind,
                "source_label": source_label,
                "effect_text": effect_text,
                "required_handling": required_handling,
            }
            for target_kind, target_subkind, source_label, effect_text, required_handling in compatibility_pairs
        ],
        "linked_modifiers": linked_modifiers,
        "modifier_sections": modifier_sections,
        "values": extracted_values,
    }


def load_local_assets(db_path: Path) -> list[LocalAsset]:
    inventory_types = set(load_inventory_asset_types(ROOT / "src" / "lib" / "inventory-assets.ts"))
    scoped_types = inventory_types | {"EnchantmentDataAsset"}

    con = sqlite3.connect(db_path)
    con.row_factory = sqlite3.Row
    rows = con.execute(
        """
        SELECT asset_guid, name, display_name, bundle, data, script_type, icon_png
        FROM assets
        WHERE script_type IN ({})
        ORDER BY asset_guid
        """.format(",".join("?" for _ in scoped_types)),
        tuple(sorted(scoped_types)),
    ).fetchall()
    con.close()

    assets: list[LocalAsset] = []
    for row in rows:
        target_kind, target_subkind = classify_target(
            row["script_type"],
            row["display_name"],
            "",
        )
        assets.append(
            LocalAsset(
                asset_guid=str(row["asset_guid"]),
                name=row["name"],
                display_name=row["display_name"],
                bundle=row["bundle"],
                data=row["data"],
                script_type=row["script_type"],
                icon_png=row["icon_png"],
                target_kind=target_kind,
                target_subkind=target_subkind,
            )
        )
    return assets


def limited_local_assets(local_assets: list[LocalAsset], limit: int | None) -> list[LocalAsset]:
    if limit is None or limit <= 0 or limit >= len(local_assets):
        return local_assets
    return local_assets[:limit]


def sample_local_assets_for_indexes(
    local_assets: list[LocalAsset],
    entities_by_path: dict[str, SiteEntity],
    sample_per_index: int,
) -> list[LocalAsset]:
    if sample_per_index <= 0:
        return local_assets

    assets_by_guid = {asset.asset_guid: asset for asset in local_assets}
    entities_by_source: dict[str, list[SiteEntity]] = {}
    for entity in entities_by_path.values():
        entities_by_source.setdefault(entity.source_url, []).append(entity)

    selected_assets: dict[str, LocalAsset] = {
        asset.asset_guid: asset
        for asset in local_assets
        if asset.script_type == "EnchantmentDataAsset"
    }

    for index_url in INDEX_PAGE_URLS:
        if index_url.endswith("/enchants"):
            continue
        matched_for_index = 0
        seen_for_index: set[str] = set()
        for entity in sorted(entities_by_source.get(index_url, []), key=lambda item: (item.guid or "", item.path)):
            if entity.guid is None or entity.guid in seen_for_index:
                continue
            asset = assets_by_guid.get(entity.guid)
            if asset is None or asset.script_type == "EnchantmentDataAsset":
                continue
            selected_assets.setdefault(asset.asset_guid, asset)
            seen_for_index.add(entity.guid)
            matched_for_index += 1
            if matched_for_index >= sample_per_index:
                break

    return sorted(selected_assets.values(), key=lambda asset: (asset.script_type != "EnchantmentDataAsset", int(asset.asset_guid)))


def enrich_enchant_table_description(description: str) -> str:
    return normalize_whitespace(description.replace("\xa0", " "))


def enchant_table_description_prefix(description: str) -> str:
    prefix = re.split(r"\[", description, maxsplit=1)[0]
    return normalize_whitespace(prefix or description)


def load_enchant_table_rows(table_path: Path) -> list[EnchantTableRow]:
    payload = json.loads(table_path.read_text(encoding="utf-8"))
    rows: list[EnchantTableRow] = []
    seen_keys: set[tuple[str, str, str, str, str]] = set()
    for raw_row in payload.get("rows", []):
        description = enrich_enchant_table_description(str(raw_row.get("Description") or ""))
        description_prefix = enchant_table_description_prefix(description)
        type_name = normalize_whitespace(str(raw_row.get("Type") or ""))
        group_name = normalize_whitespace(str(raw_row.get("Group") or ""))
        affects_stat = normalize_whitespace(str(raw_row.get("Affects Stat") or ""))
        applies_to = normalize_whitespace(str(raw_row.get("Applies To") or ""))
        only_on_items = normalize_whitespace(str(raw_row.get("Only on Items") or ""))
        dedupe_key = (normalize_text(description_prefix), type_name, group_name, applies_to, only_on_items)
        if dedupe_key in seen_keys:
            continue
        rows.append(
            EnchantTableRow(
                description=description,
                description_prefix=description_prefix,
                type_name=type_name,
                group_name=group_name,
                drop_level=normalize_whitespace(str(raw_row.get("Drop Level") or "")),
                drop_rate=normalize_whitespace(str(raw_row.get("Drop Rate") or "")),
                affects_stat=affects_stat,
                applies_to=applies_to,
                only_on_items=only_on_items,
            )
        )
        seen_keys.add(dedupe_key)
    return rows


def fetch_enchant_table(*, cache_dir: Path, offline: bool) -> list[EnchantTableRow]:
    table_path = cache_dir / "site-meta" / "enchants-table.json"
    if table_path.exists():
        return load_enchant_table_rows(table_path)
    if offline:
        raise RuntimeError(f"offline mode: missing cached enchant table at {table_path}")
    node_bin = shutil.which("node")
    if node_bin is None:
        raise RuntimeError("missing required command for enchant table scraping: node")
    subprocess.run(
        [
            node_bin,
            str(ROOT / "scripts" / "scrape_enchants_table.mjs"),
            "--output",
            str(table_path),
        ],
        cwd=ROOT,
        check=True,
    )
    return load_enchant_table_rows(table_path)


def match_enchant_table_row(detail: dict[str, Any], table_rows_by_prefix: dict[str, list[EnchantTableRow]]) -> EnchantTableRow | None:
    candidates: list[EnchantTableRow] = []
    candidate_texts: list[str] = []
    for candidate_text in (
        detail.get("enchant_display_text"),
        detail.get("effect_text"),
        detail.get("title"),
    ):
        if not isinstance(candidate_text, str):
            continue
        normalized = normalize_text(candidate_text)
        if not normalized:
            continue
        candidate_texts.append(candidate_text)
        matched_rows = table_rows_by_prefix.get(normalized, [])
        if matched_rows:
            candidates = matched_rows
            break

    if not candidates:
        normalized_candidates = [normalize_text(text) for text in candidate_texts if normalize_text(text)]
        fuzzy_matches: list[EnchantTableRow] = []
        seen_keys: set[tuple[str, str, str]] = set()
        for row_group in table_rows_by_prefix.values():
            for row in row_group:
                row_normalized = normalize_text(row.description_prefix)
                if not row_normalized:
                    continue
                if any(
                    row_normalized.startswith(candidate) or candidate.startswith(row_normalized)
                    for candidate in normalized_candidates
                ):
                    key = (row.description_prefix, row.type_name, row.only_on_items)
                    if key in seen_keys:
                        continue
                    fuzzy_matches.append(row)
                    seen_keys.add(key)
        candidates = fuzzy_matches

    if not candidates:
        return None
    if len(candidates) == 1:
        return candidates[0]

    detail_applies_to = normalize_text(str(detail.get("detail_applies_to") or ""))
    if detail_applies_to:
        filtered = [row for row in candidates if normalize_text(row.applies_to) == detail_applies_to]
        if len(filtered) == 1:
            return filtered[0]
        if filtered:
            candidates = filtered

    detail_type_hint = normalize_whitespace(str(detail.get("modifier_type_hint") or ""))
    if detail_type_hint:
        filtered = [row for row in candidates if row.type_name == detail_type_hint]
        if len(filtered) == 1:
            return filtered[0]
        if filtered:
            candidates = filtered

    title_normalized = normalize_text(str(detail.get("title") or ""))
    effect_normalized = normalize_text(str(detail.get("effect_text") or ""))
    if "onblock" in title_normalized or "onblock" in effect_normalized:
        filtered = [row for row in candidates if "onblock" in normalize_text(row.affects_stat)]
        if len(filtered) == 1:
            return filtered[0]
        if filtered:
            candidates = filtered
    elif "poisedamage" in effect_normalized:
        filtered = [row for row in candidates if "onblock" not in normalize_text(row.affects_stat)]
        if len(filtered) == 1:
            return filtered[0]
        if filtered:
            candidates = filtered

    regular_item_rows = [row for row in candidates if row.only_on_items == "Regular Item"]
    if len(regular_item_rows) == 1:
        return regular_item_rows[0]

    magical_rows = [row for row in candidates if row.type_name == "Magical"]
    if len(magical_rows) == 1:
        return magical_rows[0]

    return None


def compatibility_rows_from_enchant_table_row(row: EnchantTableRow) -> list[dict[str, str | None]]:
    rows: list[dict[str, str | None]] = []
    required_handling = normalize_handling(row.applies_to)
    for target_kind, target_subkind in normalize_target_tokens(row.applies_to):
        rows.append(
            {
                "target_kind": target_kind,
                "target_subkind": target_subkind,
                "source_label": "Applies To",
                "effect_text": row.description_prefix,
                "required_handling": required_handling,
            }
        )
    return rows


def discover_site_data(fetcher: SiteFetcher) -> tuple[dict[str, dict[str, Any]], dict[str, SiteEntity]]:
    categories: dict[str, dict[str, Any]] = {}
    entities_by_path: dict[str, SiteEntity] = {}
    with concurrent.futures.ThreadPoolExecutor(max_workers=DEFAULT_MAX_WORKERS) as pool:
        future_to_category = {
            pool.submit(fetcher.fetch, index_url, is_index=True): index_url
            for index_url in INDEX_PAGE_URLS
        }
        for future in concurrent.futures.as_completed(future_to_category):
            index_url = future_to_category[future]
            html = future.result()
            parsed = urlparse(index_url)
            category, subcategory = category_from_path(parsed.path)
            if category is None:
                continue
            category_entities = extract_site_entities(index_url, html)
            key = parsed.path.rstrip("/")
            categories[key] = {
                "site_category": category,
                "index_url": index_url,
                "subcategory": subcategory,
                "reported_count": None,
                "scraped_count": len(category_entities),
            }
            for entity in category_entities:
                entities_by_path[entity.path] = entity

    return categories, entities_by_path


def match_site_entity(asset: LocalAsset, entities_by_guid: dict[str, SiteEntity], title_index: dict[str, list[SiteEntity]]) -> tuple[SiteEntity | None, str]:
    if asset.script_type != "EnchantmentDataAsset" and asset.asset_guid in entities_by_guid:
        return entities_by_guid[asset.asset_guid], "guid"

    if asset.script_type != "EnchantmentDataAsset":
        return None, "guid_not_indexed"

    synthetic_enchant = SiteEntity(
        path=f"/db/enchants/{asset.asset_guid}",
        guid=asset.asset_guid,
        category="enchants",
        subcategory=None,
        title=asset.display_name or asset.name or asset.asset_guid,
        source_url=f"{SITE_ROOT}/db/enchants",
    )
    return synthetic_enchant, "enchant_guid_probe"


def init_output_db(db_path: Path) -> sqlite3.Connection:
    if db_path.exists():
        db_path.unlink()
    con = sqlite3.connect(db_path)
    con.executescript(
        """
        PRAGMA journal_mode = DELETE;

        CREATE TABLE assets (
            asset_guid     INTEGER PRIMARY KEY,
            name           TEXT NOT NULL,
            display_name   TEXT NOT NULL,
            bundle         TEXT,
            data           TEXT,
            script_type    TEXT,
            icon_png       BLOB,
            entry_version  TEXT NOT NULL,
            has_site_match INTEGER NOT NULL DEFAULT 0,
            site_path      TEXT,
            site_category  TEXT,
            site_title     TEXT,
            target_kind    TEXT,
            target_subkind TEXT,
            raw_site_json  TEXT
        );
        CREATE INDEX idx_assets_name ON assets(name);
        CREATE INDEX idx_assets_display_name ON assets(display_name);
        CREATE INDEX idx_assets_script_type ON assets(script_type);
        CREATE INDEX idx_assets_entry_version ON assets(entry_version);

        CREATE TABLE equipment_details (
            asset_guid         INTEGER PRIMARY KEY,
            target_kind        TEXT,
            target_subkind     TEXT,
            handling           TEXT,
            rune_slots         INTEGER,
            gem_slots          INTEGER,
            category           TEXT,
            subcategory        TEXT,
            source_text        TEXT,
            sell_value         TEXT,
            weight             TEXT,
            durability         TEXT,
            requirements       TEXT,
            damage             TEXT,
            damage_type        TEXT,
            scaling            TEXT,
            focus_cost         TEXT,
            focus_gain         TEXT,
            raw_site_json      TEXT NOT NULL
        );

        CREATE TABLE modifier_details (
            asset_guid         INTEGER PRIMARY KEY,
            modifier_kind      TEXT NOT NULL,
            title              TEXT NOT NULL,
            modifier_group     TEXT,
            affects_stat       TEXT,
            drop_level         TEXT,
            drop_rate          TEXT,
            only_on_items      TEXT,
            effect_text        TEXT,
            description_text   TEXT,
            source_text        TEXT,
            sell_value         TEXT,
            raw_site_json      TEXT NOT NULL,
            roll_kind          TEXT,
            roll_min           REAL,
            roll_max           REAL,
            roll_value         REAL,
            roll_unit          TEXT,
            roll_is_negative   INTEGER,
            roll_text          TEXT
        );
        CREATE INDEX idx_modifier_kind ON modifier_details(modifier_kind);

        CREATE TABLE modifier_compatibility (
            modifier_guid      INTEGER NOT NULL,
            target_kind        TEXT NOT NULL,
            target_subkind     TEXT,
            required_handling  TEXT,
            source_label       TEXT,
            effect_text        TEXT,
            PRIMARY KEY (modifier_guid, target_kind, target_subkind, required_handling, source_label)
        );
        CREATE INDEX idx_modifier_compatibility_kind
            ON modifier_compatibility(target_kind, target_subkind, required_handling);

        CREATE TABLE item_modifier_loadout (
            asset_guid         INTEGER NOT NULL,
            modifier_guid      INTEGER NOT NULL,
            modifier_kind      TEXT NOT NULL,
            source_label       TEXT,
            ordinal            INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (asset_guid, modifier_guid, modifier_kind, source_label)
        );
        CREATE INDEX idx_item_modifier_loadout_asset ON item_modifier_loadout(asset_guid, modifier_kind, ordinal);

        CREATE TABLE modifier_item_restrictions (
            modifier_guid      INTEGER NOT NULL,
            item_guid          INTEGER,
            item_name          TEXT NOT NULL,
            PRIMARY KEY (modifier_guid, item_name)
        );
        CREATE INDEX idx_modifier_item_restrictions_modifier
            ON modifier_item_restrictions(modifier_guid, item_guid);

        CREATE TABLE ingest_audit (
            asset_guid         INTEGER PRIMARY KEY,
            asset_type         TEXT NOT NULL,
            match_status       TEXT NOT NULL,
            site_path          TEXT,
            notes              TEXT
        );

        CREATE TABLE parse_warnings (
            asset_guid         INTEGER NOT NULL,
            warning_code       TEXT NOT NULL,
            detail             TEXT,
            PRIMARY KEY (asset_guid, warning_code)
        );

        CREATE TABLE page_text_snapshots (
            asset_guid         INTEGER PRIMARY KEY,
            site_path          TEXT NOT NULL,
            rendered_text      TEXT NOT NULL
        );

        CREATE TABLE site_category_audit (
            site_category       TEXT PRIMARY KEY,
            reported_count      INTEGER,
            scraped_count       INTEGER NOT NULL DEFAULT 0,
            matched_local_count INTEGER NOT NULL DEFAULT 0,
            status              TEXT NOT NULL
        );
        """
    )
    return con


def write_output_db(
    output_db: Path,
    local_assets: list[LocalAsset],
    categories: dict[str, dict[str, Any]],
    matches: dict[str, tuple[SiteEntity, str]],
    parsed_details: dict[str, dict[str, Any]],
    enchant_table_matches: dict[str, EnchantTableRow],
) -> None:
    con = init_output_db(output_db)
    matched_asset_guid_by_path = {
        entity.path: asset_guid
        for asset_guid, (entity, _reason) in matches.items()
    }
    local_modifier_titles: dict[str, list[str]] = {}
    local_item_titles: dict[str, list[str]] = {}
    for asset in local_assets:
        for candidate in (asset.display_name, asset.name):
            normalized = normalize_text(candidate)
            if not normalized:
                continue
            local_item_titles.setdefault(normalized, []).append(asset.asset_guid)
        if asset.script_type not in {"EnchantmentDataAsset", "HeroRuneDataAsset", "EnchantGemItemDataAsset"}:
            continue
        for candidate in (asset.display_name, asset.name):
            normalized = normalize_text(candidate)
            if not normalized:
                continue
            local_modifier_titles.setdefault(normalized, []).append(asset.asset_guid)

    resolved_modifier_titles: dict[str, list[str]] = {}
    for asset in local_assets:
        matched = matches.get(asset.asset_guid)
        site_entity = matched[0] if matched else None
        detail = parsed_details.get(site_entity.path) if site_entity else None
        if not detail:
            continue
        modifier_kind = modifier_kind_for_asset_type(asset.script_type, detail["category"])
        if modifier_kind is None:
            continue
        title_candidates = {
            detail.get("title"),
            detail.get("effect_text"),
            detail.get("fields", {}).get("Description"),
            asset.display_name,
            asset.name,
        }
        for candidate in title_candidates:
            if not isinstance(candidate, str):
                continue
            normalized = normalize_text(candidate)
            if not normalized:
                continue
            resolved_modifier_titles.setdefault(normalized, []).append(asset.asset_guid)

    for normalized, asset_guids in resolved_modifier_titles.items():
        resolved_modifier_titles[normalized] = sorted(set(asset_guids), key=int)

    matched_count_by_category: dict[str, int] = {}
    for asset in local_assets:
        matched = matches.get(asset.asset_guid)
        site_entity = matched[0] if matched else None
        match_reason = matched[1] if matched else "fallback"
        detail = parsed_details.get(site_entity.path) if site_entity else None

        if detail and asset.target_kind is None:
            target_kind, target_subkind = classify_target(
                asset.script_type,
                asset.display_name,
                site_entity.path,
            )
        else:
            target_kind, target_subkind = asset.target_kind, asset.target_subkind

        con.execute(
            """
            INSERT INTO assets (
                asset_guid, name, display_name, bundle, data, script_type, icon_png,
                entry_version, has_site_match, site_path, site_category, site_title,
                target_kind, target_subkind, raw_site_json
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                int(asset.asset_guid),
                asset.name,
                asset.display_name,
                asset.bundle,
                asset.data,
                asset.script_type,
                asset.icon_png,
                "v2" if detail else "v1",
                1 if detail else 0,
                site_entity.path if site_entity else None,
                site_entity.category if site_entity else None,
                detail["title"] if detail else None,
                target_kind,
                target_subkind,
                json.dumps(detail, ensure_ascii=True) if detail else None,
            ),
        )

        if site_entity:
            matched_count_by_category[site_entity.category] = matched_count_by_category.get(site_entity.category, 0) + 1

        if detail:
            modifier_kind = modifier_kind_for_asset_type(asset.script_type, detail["category"])
            if modifier_kind is None:
                if target_kind is None:
                    modifier_kind = None
                else:
                    values = detail["values"]
                    con.execute(
                        """
                        INSERT INTO equipment_details (
                            asset_guid, target_kind, target_subkind, handling, rune_slots, gem_slots, category, subcategory,
                            source_text, sell_value, weight, durability, requirements,
                            damage, damage_type, scaling, focus_cost, focus_gain, raw_site_json
                        )
                        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                        """,
                        (
                            int(asset.asset_guid),
                            target_kind,
                            target_subkind,
                            detail.get("handling"),
                            detail.get("rune_slots"),
                            detail.get("gem_slots"),
                            detail["category"],
                            detail["subcategory"],
                            values.get("Source"),
                            values.get("Sell Value") or values.get("Value"),
                            values.get("Weight"),
                            values.get("Durability"),
                            values.get("Requirements"),
                            values.get("Damage"),
                            values.get("Damage Type"),
                            values.get("Scaling"),
                            values.get("Focus Cost"),
                            values.get("Focus Gain"),
                            json.dumps(detail, ensure_ascii=True),
                        ),
                    )
            if modifier_kind is not None:
                values = detail["values"]
                enchant_table_row = enchant_table_matches.get(asset.asset_guid)
                con.execute(
                    """
                    INSERT INTO modifier_details (
                        asset_guid, modifier_kind, title, modifier_group, affects_stat, drop_level, drop_rate, only_on_items,
                        effect_text, description_text,
                        source_text, sell_value, raw_site_json
                    )
                    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                    """,
                    (
                        int(asset.asset_guid),
                        modifier_kind,
                        detail["title"],
                        enchant_table_row.group_name if enchant_table_row else None,
                        enchant_table_row.affects_stat if enchant_table_row else None,
                        enchant_table_row.drop_level if enchant_table_row else None,
                        enchant_table_row.drop_rate if enchant_table_row else None,
                        enchant_table_row.only_on_items if enchant_table_row else None,
                        detail.get("effect_text"),
                        enchant_table_row.description if enchant_table_row else detail["fields"].get("Description"),
                        values.get("Source"),
                        values.get("Sell Value") or values.get("Value"),
                        json.dumps(detail, ensure_ascii=True),
                    ),
                )
                compatibility_rows = detail.get("compatibility", [])
                if modifier_kind == "enchantment" and enchant_table_row is not None:
                    compatibility_rows = compatibility_rows_from_enchant_table_row(enchant_table_row)
                elif modifier_kind == "gem":
                    compatibility_rows = default_gem_compatibility_rows(detail.get("effect_text"))
                for compatibility in compatibility_rows:
                    con.execute(
                        """
                        INSERT OR IGNORE INTO modifier_compatibility (
                            modifier_guid, target_kind, target_subkind, required_handling, source_label, effect_text
                        )
                        VALUES (?, ?, ?, ?, ?, ?)
                        """,
                        (
                            int(asset.asset_guid),
                            compatibility["target_kind"],
                            compatibility["target_subkind"],
                            compatibility.get("required_handling"),
                            compatibility["source_label"],
                            compatibility["effect_text"],
                        ),
                    )
                if modifier_kind == "enchantment" and enchant_table_row is not None and enchant_table_row.only_on_items not in {"", "Regular Item"}:
                    normalized_item_name = normalize_text(enchant_table_row.only_on_items)
                    item_guid_matches = sorted(set(local_item_titles.get(normalized_item_name, [])), key=int)
                    item_guid = int(item_guid_matches[0]) if len(item_guid_matches) == 1 else None
                    con.execute(
                        """
                        INSERT OR IGNORE INTO modifier_item_restrictions (
                            modifier_guid, item_guid, item_name
                        )
                        VALUES (?, ?, ?)
                        """,
                        (
                            int(asset.asset_guid),
                            item_guid,
                            enchant_table_row.only_on_items,
                        ),
                    )

        if detail:
            for ordinal, linked_modifier in enumerate(detail.get("linked_modifiers", [])):
                modifier_guid = linked_modifier["guid"]
                if modifier_guid is None and linked_modifier["path"] in matched_asset_guid_by_path:
                    modifier_guid = matched_asset_guid_by_path[linked_modifier["path"]]
                if modifier_guid is None:
                    normalized_title = normalize_text(str(linked_modifier.get("title") or ""))
                    title_matches = resolved_modifier_titles.get(normalized_title, [])
                    if len(title_matches) != 1:
                        title_matches = local_modifier_titles.get(normalized_title, [])
                    if len(title_matches) == 1:
                        modifier_guid = title_matches[0]
                if modifier_guid is None:
                    continue
                modifier_kind = {
                    "enchants": "enchantment",
                    "runes": "rune",
                    "gems": "gem",
                }.get(linked_modifier["category"])
                if modifier_kind is None:
                    continue
                con.execute(
                    """
                    INSERT OR IGNORE INTO item_modifier_loadout (
                        asset_guid, modifier_guid, modifier_kind, source_label, ordinal
                    )
                    VALUES (?, ?, ?, ?, ?)
                    """,
                    (
                        int(asset.asset_guid),
                        int(modifier_guid),
                        modifier_kind,
                        linked_modifier["section"],
                        ordinal,
                        ),
                    )

            if detail.get("warnings"):
                for warning_code in detail["warnings"]:
                    con.execute(
                        """
                        INSERT OR IGNORE INTO parse_warnings (asset_guid, warning_code, detail)
                        VALUES (?, ?, ?)
                        """,
                        (
                            int(asset.asset_guid),
                            warning_code,
                            site_entity.path if site_entity else None,
                        ),
                    )
            if asset.script_type == "EnchantmentDataAsset" and asset.asset_guid not in enchant_table_matches:
                con.execute(
                    """
                    INSERT OR IGNORE INTO parse_warnings (asset_guid, warning_code, detail)
                    VALUES (?, ?, ?)
                    """,
                    (
                        int(asset.asset_guid),
                        "enchant_table_unmatched",
                        site_entity.path if site_entity else None,
                    ),
                )

            rendered_text = detail.get("rendered_text")
            if isinstance(rendered_text, str) and rendered_text:
                con.execute(
                    """
                    INSERT OR REPLACE INTO page_text_snapshots (asset_guid, site_path, rendered_text)
                    VALUES (?, ?, ?)
                    """,
                    (
                        int(asset.asset_guid),
                        site_entity.path if site_entity else "",
                        rendered_text,
                    ),
                )

        con.execute(
            """
            INSERT INTO ingest_audit (asset_guid, asset_type, match_status, site_path, notes)
            VALUES (?, ?, ?, ?, ?)
            """,
            (
                int(asset.asset_guid),
                asset.script_type,
                "matched" if detail else match_reason,
                site_entity.path if site_entity else None,
                match_reason,
            ),
        )

    for category_key, info in categories.items():
        reported = info.get("reported_count")
        scraped = info.get("scraped_count", 0)
        site_category = str(info.get("site_category") or category_key)
        matched_local = matched_count_by_category.get(site_category, 0)
        if reported is None:
            status = "missing_reported_count"
        elif scraped < reported:
            status = "partial_scrape"
        elif matched_local == 0:
            status = "scraped_no_matches"
        else:
            status = "ok"
        con.execute(
            """
            INSERT INTO site_category_audit (
                site_category, reported_count, scraped_count, matched_local_count, status
            )
            VALUES (?, ?, ?, ?, ?)
            """,
            (category_key, reported, scraped, matched_local, status),
        )

    con.commit()
    con.close()


def build_catalog(
    *,
    input_db: Path,
    output_db: Path,
    cache_dir: Path,
    offline: bool,
    limit: int | None,
    sample_per_index: int | None,
    max_workers: int,
    min_delay: float,
    max_delay: float,
    user_agent: str,
) -> None:
    local_assets = load_local_assets(input_db)
    archive = PageArchive(cache_dir)
    fetcher = SiteFetcher(
        archive,
        offline=offline,
        max_workers=max_workers,
        min_delay=min_delay,
        max_delay=max_delay,
        user_agent=user_agent,
    )

    try:
        categories, entities_by_path = discover_site_data(fetcher)
        enchant_table_rows = fetch_enchant_table(cache_dir=cache_dir, offline=offline)
        if sample_per_index is not None:
            local_assets = sample_local_assets_for_indexes(local_assets, entities_by_path, sample_per_index)
        else:
            local_assets = limited_local_assets(local_assets, limit)
        entities_by_guid = {entity.guid: entity for entity in entities_by_path.values() if entity.guid}
        title_index: dict[str, list[SiteEntity]] = {}
        for entity in entities_by_path.values():
            normalized = normalize_text(entity.title)
            title_index.setdefault(normalized, []).append(entity)

        matches: dict[str, tuple[SiteEntity, str]] = {}
        enchant_entities_to_fetch: dict[str, SiteEntity] = {}
        non_enchant_entities_to_fetch: dict[str, SiteEntity] = {}
        for asset in local_assets:
            entity, reason = match_site_entity(asset, entities_by_guid, title_index)
            if entity is not None:
                matches[asset.asset_guid] = (entity, reason)
                target_bucket = enchant_entities_to_fetch if entity.category == "enchants" else non_enchant_entities_to_fetch
                target_bucket[entity.path] = entity

        parsed_details: dict[str, dict[str, Any]] = {}
        with concurrent.futures.ThreadPoolExecutor(max_workers=max_workers) as pool:
            future_to_entity: dict[concurrent.futures.Future[tuple[str | None, int | None]], SiteEntity] = {}
            for entity in enchant_entities_to_fetch.values():
                future = pool.submit(
                    fetcher.fetch_optional,
                    urljoin(SITE_ROOT, entity.path),
                    guid=entity.guid,
                    is_index=False,
                )
                future_to_entity[future] = entity
            for future in concurrent.futures.as_completed(future_to_entity):
                entity = future_to_entity[future]
                html, status = future.result()
                if html is None:
                    reason = "enchant_guid_404" if status == 404 else "enchant_probe_unavailable"
                    for asset_guid, (matched_entity, _match_reason) in list(matches.items()):
                        if matched_entity.path == entity.path:
                            matches[asset_guid] = (matched_entity, reason)
                    continue
                parsed_details[entity.path] = parse_detail_page(entity, html)

        with concurrent.futures.ThreadPoolExecutor(max_workers=max_workers) as pool:
            future_to_entity_str: dict[concurrent.futures.Future[str], SiteEntity] = {}
            for entity in non_enchant_entities_to_fetch.values():
                future = pool.submit(
                    fetcher.fetch,
                    urljoin(SITE_ROOT, entity.path),
                    guid=entity.guid,
                    is_index=False,
                )
                future_to_entity_str[future] = entity
            for future in concurrent.futures.as_completed(future_to_entity_str):
                entity = future_to_entity_str[future]
                html = future.result()
                parsed_details[entity.path] = parse_detail_page(entity, html)

        enchant_table_by_prefix: dict[str, list[EnchantTableRow]] = {}
        for row in enchant_table_rows:
            enchant_table_by_prefix.setdefault(normalize_text(row.description_prefix), []).append(row)

        enchant_table_matches: dict[str, EnchantTableRow] = {}
        for asset in local_assets:
            if asset.script_type != "EnchantmentDataAsset":
                continue
            matched = matches.get(asset.asset_guid)
            site_entity = matched[0] if matched else None
            detail = parsed_details.get(site_entity.path) if site_entity else None
            if not detail:
                continue
            table_row = match_enchant_table_row(detail, enchant_table_by_prefix)
            if table_row is not None:
                enchant_table_matches[asset.asset_guid] = table_row

        write_output_db(output_db, local_assets, categories, matches, parsed_details, enchant_table_matches)
    finally:
        fetcher.close()


def main() -> None:
    parser = argparse.ArgumentParser(description="Build inventory-focused catalog 2.0")
    parser.add_argument("--input-db", type=Path, default=DEFAULT_INPUT_DB, help="Raw bundle catalog input")
    parser.add_argument("--output-db", type=Path, default=DEFAULT_OUTPUT_DB, help="Catalog 2.0 output")
    parser.add_argument("--cache-dir", type=Path, default=DEFAULT_CACHE_DIR, help="Cache/archive directory")
    parser.add_argument("--offline", action="store_true", help="Use only archived pages; fail on cache miss")
    parser.add_argument("--limit", type=int, default=None, help="Limit the number of scoped local assets to process")
    parser.add_argument(
        "--sample-per-index",
        type=int,
        default=None,
        help="Process all scoped enchantments plus up to N GUID-matched local assets per index page",
    )
    parser.add_argument("--max-workers", type=int, default=DEFAULT_MAX_WORKERS, help="Maximum concurrent requests")
    parser.add_argument("--min-delay", type=float, default=DEFAULT_MIN_DELAY, help="Minimum per-request sleep")
    parser.add_argument("--max-delay", type=float, default=DEFAULT_MAX_DELAY, help="Maximum per-request sleep")
    parser.add_argument(
        "--user-agent",
        default="nrftw-save-editor-catalog2-bot/0.1 (+https://github.com/openai/codex)",
        help="User-Agent for site requests",
    )
    args = parser.parse_args()

    if not args.input_db.is_file():
        raise SystemExit(f"Missing input DB: {args.input_db}")
    args.output_db.parent.mkdir(parents=True, exist_ok=True)
    args.cache_dir.mkdir(parents=True, exist_ok=True)

    build_catalog(
        input_db=args.input_db,
        output_db=args.output_db,
        cache_dir=args.cache_dir,
        offline=args.offline,
        limit=args.limit,
        sample_per_index=args.sample_per_index,
        max_workers=max(1, args.max_workers),
        min_delay=max(0.0, args.min_delay),
        max_delay=max(args.min_delay, args.max_delay),
        user_agent=args.user_agent,
    )

    print(f"Built catalog 2.0: {args.output_db}")
    print(f"Archived pages: {args.cache_dir}")


if __name__ == "__main__":
    main()

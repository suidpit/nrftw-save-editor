/**
 * catalog.ts — sqlite-wasm based catalog queries.
 * Uses @sqlite.org/sqlite-wasm with sqlite3_deserialize to load the DB in-memory.
 */

import type { Database, SqlValue, Sqlite3Static } from "@sqlite.org/sqlite-wasm";
import { writable } from "svelte/store";

export const catalogLoaded = writable(false);

export interface AssetDetail {
  guid: string;
  name: string;
  displayName: string;
  bundle: string | null;
  scriptType: string | null;
  icon: string | null;
  data: Record<string, unknown> | null;
  entryVersion?: "v1" | "v2" | null;
  hasSiteMatch?: boolean;
  sitePath?: string | null;
  siteCategory?: string | null;
  siteTitle?: string | null;
  targetKind?: string | null;
  targetSubkind?: string | null;
}

export interface CatalogRow {
  guid: string;
  name: string;
  displayName: string;
  scriptType: string | null;
  icon: string | null;
}

export interface EquipmentTarget {
  guid: string;
  targetKind: string | null;
  targetSubkind: string | null;
  handling: string | null;
  runeSlots: number | null;
  gemSlots: number | null;
}

export interface CompatibleModifierRow extends CatalogRow {
  modifierKind: string | null;
  effectText: string | null;
  sourceLabel: string | null;
  targetKind: string;
  targetSubkind: string | null;
  requiredHandling: string | null;
}

export interface ItemModifierLoadoutRow extends CatalogRow {
  modifierKind: string;
  sourceLabel: string | null;
  ordinal: number;
}

export interface CatalogEntryMeta {
  guid: string;
  entryVersion: "v1" | "v2" | null;
  hasSiteMatch: boolean;
  sitePath: string | null;
  siteCategory: string | null;
  siteTitle: string | null;
  targetKind: string | null;
  targetSubkind: string | null;
}

export interface ModifierDetail extends CatalogRow {
  modifierKind: string | null;
  title: string;
  modifierGroup: string | null;
  affectsStat: string | null;
  dropLevel: string | null;
  dropRate: string | null;
  onlyOnItems: string | null;
  effectText: string | null;
  descriptionText: string | null;
  sourceText: string | null;
  isFacet: boolean;
}

export interface CompatibleEnchantmentRow extends ModifierDetail {
  sourceLabel: string | null;
  targetKind: string;
  targetSubkind: string | null;
  requiredHandling: string | null;
  restrictedToItemNames: string[];
}

// ── Module state ──────────────────────────────────────────────────────────────

let db: Database | null = null;
let catalogPromise: Promise<void> | null = null;
const assetCache = new Map<string, AssetDetail | null>();
const iconCache = new Map<string, string | null>();
const equipmentTargetCache = new Map<string, EquipmentTarget | null>();
let assetQueryCount = 0;
let hasCatalogV2Tables: boolean | null = null;

// ── Init ──────────────────────────────────────────────────────────────────────

export function loadCatalog(): Promise<void> {
  if (!catalogPromise) {
    catalogPromise = _loadCatalog().catch((error) => {
      catalogPromise = null;
      catalogLoaded.set(false);
      throw error;
    });
  }
  return catalogPromise;
}

function concatChunks(chunks: Uint8Array[]): Uint8Array {
  const totalLength = chunks.reduce((sum, chunk) => sum + chunk.byteLength, 0);
  const combined = new Uint8Array(totalLength);
  let offset = 0;
  for (const chunk of chunks) {
    combined.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return combined;
}

function isSqliteDatabase(bytes: Uint8Array): boolean {
  if (bytes.byteLength < 16) {
    return false;
  }

  const header = new TextDecoder("ascii").decode(bytes.subarray(0, 16));
  return header === "SQLite format 3\0";
}

async function fetchCatalogBytes(base: string): Promise<Uint8Array> {
  const singleUrl = `${base}catalog.db`;
  const singleRes = await fetch(singleUrl);
  if (singleRes.ok) {
    const bytes = new Uint8Array(await singleRes.arrayBuffer());
    if (isSqliteDatabase(bytes)) {
      return bytes;
    }
  }

  const manifestUrl = `${base}catalog.db.parts.json`;
  const manifestRes = await fetch(manifestUrl);
  if (!manifestRes.ok) {
    throw new Error(`fetch catalog.db: ${singleRes.status}`);
  }

  const manifest = await manifestRes.json() as {
    parts?: unknown;
  };
  if (!Array.isArray(manifest.parts) || manifest.parts.length === 0) {
    throw new Error("catalog.db.parts.json is missing a valid parts array");
  }

  const chunkResponses = await Promise.all(
    manifest.parts.map(async (part): Promise<Uint8Array> => {
      if (typeof part !== "string" || !part) {
        throw new Error("catalog.db.parts.json contains an invalid part entry");
      }

      const res = await fetch(`${base}${part}`);
      if (!res.ok) {
        throw new Error(`fetch ${part}: ${res.status}`);
      }
      return new Uint8Array(await res.arrayBuffer());
    }),
  );

  const bytes = concatChunks(chunkResponses);
  if (!isSqliteDatabase(bytes)) {
    throw new Error("combined catalog.db chunks did not produce a valid SQLite database");
  }

  return bytes;
}

async function _loadCatalog(): Promise<void> {
  const base = import.meta.env.BASE_URL ?? "/";
  const dbBytes = await fetchCatalogBytes(base);

  const { default: sqlite3InitModule } =
    await import("@sqlite.org/sqlite-wasm");
  const sqlite3: Sqlite3Static = await sqlite3InitModule({
    print: () => {},
    printErr: console.error,
  });

  // Allocate memory for DB bytes and deserialize into an in-memory DB
  const p = sqlite3.wasm.allocFromTypedArray(dbBytes);
  db = new sqlite3.oo1.DB(":memory:");
  sqlite3.capi.sqlite3_deserialize(
    db,
    "main",
    p,
    dbBytes.byteLength,
    dbBytes.byteLength,
    sqlite3.capi.SQLITE_DESERIALIZE_FREEONCLOSE |
      sqlite3.capi.SQLITE_DESERIALIZE_READONLY,
  );

  assetCache.clear();
  iconCache.clear();
  equipmentTargetCache.clear();
  hasCatalogV2Tables = null;
  catalogLoaded.set(true);
}

// ── Query helpers ─────────────────────────────────────────────────────────────

/** Safe for any blob size — avoids stack overflow from spreading large arrays. */
function iconToDataUrl(iconBlob: Uint8Array | null): string | null {
  if (!iconBlob) return null;
  let binary = "";
  for (let i = 0; i < iconBlob.length; i++) {
    binary += String.fromCharCode(iconBlob[i]);
  }
  return `data:image/png;base64,${btoa(binary)}`;
}

function cachedIcon(assetGuid: string, iconBlob: Uint8Array | null): string | null {
  if (iconCache.has(assetGuid)) {
    return iconCache.get(assetGuid) ?? null;
  }

  const icon = iconToDataUrl(iconBlob);
  iconCache.set(assetGuid, icon);
  return icon;
}

function parseBigIntGuid(guid: string): bigint | null {
  try {
    return BigInt(guid);
  } catch {
    return null;
  }
}

type Row = Record<string, SqlValue>;

function queryObjects(sql: string, bind?: Array<string | number | bigint>): Row[] {
  if (!db) return [];
  return db.exec({
    sql,
    bind,
    rowMode: "object",
    returnValue: "resultRows",
  }) as Row[];
}

function ensureCatalogV2Tables(): boolean {
  if (hasCatalogV2Tables !== null) {
    return hasCatalogV2Tables;
  }
  const v2Tables = [
    "equipment_details",
    "modifier_details",
    "modifier_compatibility",
    "item_modifier_loadout",
    "modifier_item_restrictions",
  ];
  const rows = queryObjects(
    `SELECT COUNT(DISTINCT name) AS cnt FROM sqlite_master
     WHERE type = 'table' AND name IN (${v2Tables.map(() => "?").join(",")})`,
    v2Tables,
  );
  hasCatalogV2Tables = Number(rows[0]?.cnt ?? 0) === v2Tables.length;
  return hasCatalogV2Tables;
}

// ── Row mapping helpers ──────────────────────────────────────────────────────

function str(v: SqlValue): string {
  return typeof v === "string" ? v : "";
}

function strOrNull(v: SqlValue): string | null {
  return typeof v === "string" ? v : null;
}

function numOrNull(v: SqlValue): number | null {
  if (typeof v === "number") return v;
  if (v == null) return null;
  return Number(v);
}

function iconFromRow(row: Row, guidKey = "asset_guid", iconKey = "icon_png"): string | null {
  return cachedIcon(
    String(row[guidKey] ?? ""),
    row[iconKey] instanceof Uint8Array ? row[iconKey] : null,
  );
}

function mapCatalogRow(row: Row): CatalogRow {
  return {
    guid: String(row.asset_guid),
    name: str(row.name),
    displayName: str(row.display_name),
    scriptType: strOrNull(row.script_type),
    icon: iconFromRow(row),
  };
}

// ── Public API ────────────────────────────────────────────────────────────────

export function getScriptTypes(): string[] {
  return queryObjects(
    "SELECT DISTINCT script_type FROM assets WHERE script_type IS NOT NULL ORDER BY script_type",
  )
    .map((row) => row.script_type)
    .filter((value): value is string => typeof value === "string");
}

export function searchCatalog(
  query: string,
  limit = 50,
  scriptType = "",
): CatalogRow[] {
  if (!db) return [];

  const conditions: string[] = [];
  const params: (string | number)[] = [];

  if (query) {
    conditions.push(
      "(display_name LIKE ? OR name LIKE ? OR CAST(asset_guid AS TEXT) LIKE ?)",
    );
    const like = `%${query}%`;
    params.push(like, like, like);
  }
  if (scriptType) {
    conditions.push("script_type = ?");
    params.push(scriptType);
  }
  const where = conditions.length ? `WHERE ${conditions.join(" AND ")}` : "";
  params.push(limit);

  return queryObjects(
    `SELECT asset_guid, name, display_name, script_type, icon_png FROM assets ${where} LIMIT ?`,
    params,
  ).map(mapCatalogRow);
}

export function getAssetByGuid(guid: string): AssetDetail | null {
  if (!db) {
    return null;
  }

  if (assetCache.has(guid)) {
    return assetCache.get(guid) ?? null;
  }

  const guidValue = parseBigIntGuid(guid);
  if (guidValue === null) {
    assetCache.set(guid, null);
    return null;
  }

  assetQueryCount += 1;
  const row = queryObjects(
    `SELECT
      asset_guid, name, display_name, bundle, script_type, icon_png,
      data, entry_version, has_site_match, site_path, site_category,
      site_title, target_kind, target_subkind
     FROM assets
     WHERE asset_guid = ?`,
    [guidValue],
  )[0];
  if (!row) {
    assetCache.set(guid, null);
    return null;
  }

  const entryVersion =
    row.entry_version === "v1" || row.entry_version === "v2" ? row.entry_version : null;
  const result: AssetDetail = {
    guid: String(row.asset_guid),
    name: str(row.name),
    displayName: str(row.display_name),
    bundle: strOrNull(row.bundle),
    scriptType: strOrNull(row.script_type),
    icon: iconFromRow(row),
    data: typeof row.data === "string" ? JSON.parse(row.data) : null,
    entryVersion,
    hasSiteMatch: row.has_site_match === 1,
    sitePath: strOrNull(row.site_path),
    siteCategory: strOrNull(row.site_category),
    siteTitle: strOrNull(row.site_title),
    targetKind: strOrNull(row.target_kind),
    targetSubkind: strOrNull(row.target_subkind),
  };

  assetCache.set(guid, result);
  return result;
}

function deriveFallbackTarget(scriptType: string | null | undefined): Pick<EquipmentTarget, "targetKind" | "targetSubkind" | "handling"> {
  switch (scriptType) {
    case "WeaponStaticDataAsset":
      return { targetKind: "weapon", targetSubkind: null, handling: null };
    case "HelmDataAsset":
      return { targetKind: "helm", targetSubkind: null, handling: null };
    case "BodyDataAsset":
      return { targetKind: "body", targetSubkind: null, handling: null };
    case "PantsDataAsset":
      return { targetKind: "pants", targetSubkind: null, handling: null };
    case "GlovesDataAsset":
      return { targetKind: "gloves", targetSubkind: null, handling: null };
    case "RingsDataAsset":
      return { targetKind: "ring", targetSubkind: null, handling: null };
    default:
      return { targetKind: null, targetSubkind: null, handling: null };
  }
}

export function getCatalogEntryMeta(guid: string): CatalogEntryMeta | null {
  const asset = getAssetByGuid(guid);
  if (!asset) {
    return null;
  }

  return {
    guid: asset.guid,
    entryVersion: asset.entryVersion ?? null,
    hasSiteMatch: asset.hasSiteMatch ?? false,
    sitePath: asset.sitePath ?? null,
    siteCategory: asset.siteCategory ?? null,
    siteTitle: asset.siteTitle ?? null,
    targetKind: asset.targetKind ?? null,
    targetSubkind: asset.targetSubkind ?? null,
  };
}

export function getSiteUrl(sitePath: string | null | undefined): string | null {
  if (!sitePath) {
    return null;
  }
  return `https://www.norestforthewicked.gg${sitePath}`;
}

export function getEquipmentTargetByGuid(guid: string): EquipmentTarget | null {
  if (!db || !ensureCatalogV2Tables()) {
    return null;
  }

  if (equipmentTargetCache.has(guid)) {
    return equipmentTargetCache.get(guid) ?? null;
  }

  const guidValue = parseBigIntGuid(guid);
  if (guidValue === null) {
    equipmentTargetCache.set(guid, null);
    return null;
  }

  const row = queryObjects(
    `SELECT asset_guid, target_kind, target_subkind, handling, rune_slots, gem_slots
     FROM equipment_details
     WHERE asset_guid = ?`,
    [guidValue],
  )[0];
  if (!row) {
    equipmentTargetCache.set(guid, null);
    return null;
  }

  const result: EquipmentTarget = {
    guid: String(row.asset_guid),
    targetKind: strOrNull(row.target_kind),
    targetSubkind: strOrNull(row.target_subkind),
    handling: strOrNull(row.handling),
    runeSlots: numOrNull(row.rune_slots),
    gemSlots: numOrNull(row.gem_slots),
  };
  equipmentTargetCache.set(guid, result);
  return result;
}

export function getEquipmentTargetForEditing(
  guid: string,
  scriptType: string | null | undefined,
): EquipmentTarget | null {
  const exact = getEquipmentTargetByGuid(guid);
  if (exact) {
    return exact;
  }

  const fallback = deriveFallbackTarget(scriptType);
  if (!fallback.targetKind) {
    return null;
  }

  return {
    guid,
    targetKind: fallback.targetKind,
    targetSubkind: fallback.targetSubkind,
    handling: fallback.handling,
    runeSlots: null,
    gemSlots: null,
  };
}

export function getCompatibleModifiersForAsset(guid: string, limit = 200): CompatibleModifierRow[] {
  if (!db || !ensureCatalogV2Tables()) {
    return [];
  }

  const guidValue = parseBigIntGuid(guid);
  if (guidValue === null) {
    return [];
  }

  return queryObjects(
    `
    SELECT
      a.asset_guid,
      a.name,
      a.display_name,
      a.script_type,
      a.icon_png,
      md.modifier_kind,
      mc.effect_text,
      mc.source_label,
      mc.target_kind,
      mc.target_subkind,
      mc.required_handling
    FROM equipment_details ed
    JOIN assets current_asset
      ON current_asset.asset_guid = ed.asset_guid
    JOIN modifier_compatibility mc
      ON mc.target_kind = ed.target_kind
     AND (mc.target_subkind IS NULL OR mc.target_subkind = ed.target_subkind)
     AND (mc.required_handling IS NULL OR mc.required_handling = ed.handling)
    JOIN modifier_details md
      ON md.asset_guid = mc.modifier_guid
    JOIN assets a
      ON a.asset_guid = md.asset_guid
    WHERE ed.asset_guid = ?
      AND (
        NOT EXISTS (
          SELECT 1
          FROM modifier_item_restrictions mir
          WHERE mir.modifier_guid = mc.modifier_guid
        )
        OR EXISTS (
          SELECT 1
          FROM modifier_item_restrictions mir
          WHERE mir.modifier_guid = mc.modifier_guid
            AND (
              mir.item_guid = ed.asset_guid
              OR mir.item_name = current_asset.display_name
              OR mir.item_name = current_asset.name
            )
        )
      )
    ORDER BY a.display_name, a.name
    LIMIT ?
    `,
    [guidValue, limit],
  ).map((row) => ({
    ...mapCatalogRow(row),
    modifierKind: strOrNull(row.modifier_kind),
    effectText: strOrNull(row.effect_text),
    sourceLabel: strOrNull(row.source_label),
    targetKind: str(row.target_kind),
    targetSubkind: strOrNull(row.target_subkind),
    requiredHandling: strOrNull(row.required_handling),
  }));
}

export function getModifierDetailByGuid(guid: string): ModifierDetail | null {
  if (!db || !ensureCatalogV2Tables()) {
    return null;
  }

  const guidValue = parseBigIntGuid(guid);
  if (guidValue === null) {
    return null;
  }

  const row = queryObjects(
    `
    SELECT
      a.asset_guid,
      a.name,
      a.display_name,
      a.script_type,
      a.icon_png,
      md.modifier_kind,
      md.title,
      md.modifier_group,
      md.affects_stat,
      md.drop_level,
      md.drop_rate,
      md.only_on_items,
      md.effect_text,
      md.description_text,
      md.source_text
    FROM modifier_details md
    JOIN assets a
      ON a.asset_guid = md.asset_guid
    WHERE md.asset_guid = ?
    LIMIT 1
    `,
    [guidValue],
  )[0];

  if (!row) {
    return null;
  }

  const onlyOnItems = strOrNull(row.only_on_items);
  return {
    ...mapCatalogRow(row),
    modifierKind: strOrNull(row.modifier_kind),
    title: str(row.title),
    modifierGroup: strOrNull(row.modifier_group),
    affectsStat: strOrNull(row.affects_stat),
    dropLevel: strOrNull(row.drop_level),
    dropRate: strOrNull(row.drop_rate),
    onlyOnItems,
    effectText: strOrNull(row.effect_text),
    descriptionText: strOrNull(row.description_text),
    sourceText: strOrNull(row.source_text),
    isFacet: onlyOnItems !== null && onlyOnItems !== "Regular Item",
  };
}

// Unit Separator (ASCII 31) — safe delimiter that won't appear in item names
const RESTRICTION_DELIM = "\x1F";

export function searchCompatibleEnchantmentsForAsset(
  guid: string,
  query: string,
  limit = 50,
): CompatibleEnchantmentRow[] {
  if (!db || !ensureCatalogV2Tables()) {
    return [];
  }

  const asset = getAssetByGuid(guid);
  if (!asset) {
    return [];
  }

  const target = getEquipmentTargetForEditing(guid, asset.scriptType);
  if (!target?.targetKind) {
    return [];
  }

  const guidValue = parseBigIntGuid(guid);
  if (guidValue === null) {
    return [];
  }

  const hasExactEquipment = getEquipmentTargetByGuid(guid) !== null;
  const conditions = [
    "mc.target_kind = ?",
    "md.modifier_kind = 'enchantment'",
    `(NOT EXISTS (
        SELECT 1 FROM modifier_item_restrictions mirx
        WHERE mirx.modifier_guid = mc.modifier_guid
      ) OR EXISTS (
        SELECT 1 FROM modifier_item_restrictions mirx
        WHERE mirx.modifier_guid = mc.modifier_guid
          AND (
            mirx.item_guid = ?
            OR mirx.item_name = ?
            OR mirx.item_name = ?
          )
      ))`,
  ];
  const params: Array<string | number | bigint> = [
    target.targetKind,
    guidValue,
    asset.displayName,
    asset.name,
  ];

  if (hasExactEquipment) {
    conditions.push("(mc.target_subkind IS NULL OR mc.target_subkind = ?)");
    conditions.push("(mc.required_handling IS NULL OR mc.required_handling = ?)");
    params.push(target.targetSubkind ?? "", target.handling ?? "");
  }

  if (query.trim()) {
    const like = `%${query.trim()}%`;
    conditions.push("(a.display_name LIKE ? OR a.name LIKE ? OR md.title LIKE ? OR md.effect_text LIKE ?)");
    params.push(like, like, like, like);
  }

  params.push(limit);

  return queryObjects(
    `
    SELECT
      a.asset_guid,
      a.name,
      a.display_name,
      a.script_type,
      a.icon_png,
      md.modifier_kind,
      md.title,
      md.modifier_group,
      md.affects_stat,
      md.drop_level,
      md.drop_rate,
      md.only_on_items,
      md.effect_text,
      md.description_text,
      md.source_text,
      mc.source_label,
      mc.target_kind,
      mc.target_subkind,
      mc.required_handling,
      COALESCE(GROUP_CONCAT(mir.item_name, '${RESTRICTION_DELIM}'), '') AS restriction_names
    FROM modifier_compatibility mc
    JOIN modifier_details md
      ON md.asset_guid = mc.modifier_guid
    JOIN assets a
      ON a.asset_guid = md.asset_guid
    LEFT JOIN modifier_item_restrictions mir
      ON mir.modifier_guid = mc.modifier_guid
    WHERE ${conditions.join(" AND ")}
    GROUP BY
      a.asset_guid,
      a.name,
      a.display_name,
      a.script_type,
      a.icon_png,
      md.modifier_kind,
      md.title,
      md.modifier_group,
      md.affects_stat,
      md.drop_level,
      md.drop_rate,
      md.only_on_items,
      md.effect_text,
      md.description_text,
      md.source_text,
      mc.source_label,
      mc.target_kind,
      mc.target_subkind,
      mc.required_handling
    ORDER BY
      CASE WHEN md.only_on_items = 'Regular Item' THEN 0 ELSE 1 END,
      a.display_name,
      a.name
    LIMIT ?
    `,
    params,
  ).map((row) => {
    const onlyOnItems = strOrNull(row.only_on_items);
    const restrictionText = typeof row.restriction_names === "string" ? row.restriction_names : "";
    const restrictedToItemNames = restrictionText
      ? restrictionText.split(RESTRICTION_DELIM).filter((value, index, all) => value && all.indexOf(value) === index)
      : [];

    return {
      ...mapCatalogRow(row),
      modifierKind: strOrNull(row.modifier_kind),
      title: str(row.title),
      modifierGroup: strOrNull(row.modifier_group),
      affectsStat: strOrNull(row.affects_stat),
      dropLevel: strOrNull(row.drop_level),
      dropRate: strOrNull(row.drop_rate),
      onlyOnItems,
      effectText: strOrNull(row.effect_text),
      descriptionText: strOrNull(row.description_text),
      sourceText: strOrNull(row.source_text),
      sourceLabel: strOrNull(row.source_label),
      targetKind: str(row.target_kind),
      targetSubkind: strOrNull(row.target_subkind),
      requiredHandling: strOrNull(row.required_handling),
      restrictedToItemNames,
      isFacet: onlyOnItems !== null && onlyOnItems !== "Regular Item",
    };
  });
}

export function getPreexistingModifiersForAsset(
  guid: string,
  modifierKind = "",
  limit = 50,
): ItemModifierLoadoutRow[] {
  if (!db || !ensureCatalogV2Tables()) {
    return [];
  }

  const guidValue = parseBigIntGuid(guid);
  if (guidValue === null) {
    return [];
  }

  const conditions = ["iml.asset_guid = ?"];
  const params: Array<string | number | bigint> = [guidValue];
  if (modifierKind) {
    conditions.push("iml.modifier_kind = ?");
    params.push(modifierKind);
  }
  params.push(limit);

  return queryObjects(
    `
    SELECT
      a.asset_guid,
      a.name,
      a.display_name,
      a.script_type,
      a.icon_png,
      iml.modifier_kind,
      iml.source_label,
      iml.ordinal
    FROM item_modifier_loadout iml
    JOIN assets a
      ON a.asset_guid = iml.modifier_guid
    WHERE ${conditions.join(" AND ")}
    ORDER BY iml.ordinal, a.display_name, a.name
    LIMIT ?
    `,
    params,
  ).map((row) => ({
    ...mapCatalogRow(row),
    modifierKind: str(row.modifier_kind),
    sourceLabel: strOrNull(row.source_label),
    ordinal: typeof row.ordinal === "number" ? row.ordinal : Number(row.ordinal ?? 0),
  }));
}

export function hasCatalogPreexistingModifiers(guid: string, modifierKind = ""): boolean {
  return getPreexistingModifiersForAsset(guid, modifierKind, 1).length > 0;
}

export function __resetCatalogTestState(): void {
  assetCache.clear();
  iconCache.clear();
  equipmentTargetCache.clear();
  assetQueryCount = 0;
  hasCatalogV2Tables = null;
}

export function __getCatalogTestState(): {
  assetCacheSize: number;
  iconCacheSize: number;
  assetQueryCount: number;
} {
  return {
    assetCacheSize: assetCache.size,
    iconCacheSize: iconCache.size,
    assetQueryCount,
  };
}

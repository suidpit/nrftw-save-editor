/**
 * catalog.ts — sqlite-wasm based catalog queries.
 * Uses @sqlite.org/sqlite-wasm with sqlite3_deserialize to load the DB in-memory.
 */

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
}

export interface CatalogRow {
  guid: string;
  name: string;
  displayName: string;
  scriptType: string | null;
  icon: string | null;
}

// ── Module state ──────────────────────────────────────────────────────────────

// eslint-disable-next-line @typescript-eslint/no-explicit-any
let db: any = null;
let catalogPromise: Promise<void> | null = null;

// ── Init ──────────────────────────────────────────────────────────────────────

export function loadCatalog(): Promise<void> {
  if (!catalogPromise) {
    catalogPromise = _loadCatalog();
  }
  return catalogPromise;
}

async function _loadCatalog(): Promise<void> {
  const base = import.meta.env.BASE_URL ?? "/";
  const res = await fetch(`${base}catalog.db`);
  if (!res.ok) throw new Error(`fetch catalog.db: ${res.status}`);
  const dbBytes = new Uint8Array(await res.arrayBuffer());

  const { default: sqlite3InitModule } =
    await import("@sqlite.org/sqlite-wasm");
  const sqlite3 = await sqlite3InitModule({
    print: () => {},
    printErr: console.error,
  });

  // Allocate memory for DB bytes and deserialize into an in-memory DB
  const p = sqlite3.wasm.allocFromTypedArray(dbBytes);
  db = new sqlite3.oo1.DB();
  sqlite3.capi.sqlite3_deserialize(
    db.pointer,
    "main",
    p,
    dbBytes.byteLength,
    dbBytes.byteLength,
    sqlite3.capi.SQLITE_DESERIALIZE_FREEONCLOSE,
  );

  catalogLoaded.set(true);
}

// ── Query helpers ─────────────────────────────────────────────────────────────

function iconToDataUrl(iconBlob: unknown): string | null {
  if (!iconBlob) return null;
  // sqlite-wasm returns blobs as Uint8Array
  if (iconBlob instanceof Uint8Array) {
    const b64 = btoa(String.fromCharCode(...iconBlob));
    return `data:image/png;base64,${b64}`;
  }
  return null;
}

// ── Public API ────────────────────────────────────────────────────────────────

export function getScriptTypes(): string[] {
  if (!db) return [];
  const rows: string[] = [];
  db.exec({
    sql: "SELECT DISTINCT script_type FROM assets WHERE script_type IS NOT NULL ORDER BY script_type",
    callback: (row: [string]) => rows.push(row[0]),
  });
  return rows;
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

  const rows: CatalogRow[] = [];
  db.exec({
    sql: `SELECT asset_guid, name, display_name, script_type, icon_png FROM assets ${where} LIMIT ?`,
    bind: params,
    callback: (
      row: [string | number, string, string, string | null, Uint8Array | null],
    ) => {
      rows.push({
        guid: String(row[0]),
        name: row[1],
        displayName: row[2],
        scriptType: row[3],
        icon: iconToDataUrl(row[4]),
      });
    },
  });
  return rows;
}

export function getAssetByGuid(guid: string): AssetDetail | null {
  if (!db) return null;
  let result: AssetDetail | null = null;
  db.exec({
    sql: "SELECT asset_guid, name, display_name, bundle, script_type, icon_png, data FROM assets WHERE CAST(asset_guid AS TEXT) = ?",
    bind: [guid],
    callback: (
      row: [
        number,
        string,
        string,
        string | null,
        string | null,
        Uint8Array | null,
        string | null,
      ],
    ) => {
      result = {
        guid: String(row[0]),
        name: row[1],
        displayName: row[2],
        bundle: row[3],
        scriptType: row[4],
        icon: iconToDataUrl(row[5]),
        data: row[6] ? JSON.parse(row[6]) : null,
      };
    },
  });
  return result;
}

// Wraps the Rust WASM binary (nrftw_wasm) for save file parsing/patching.

import init, {
  parse_save,
  get_node_children,
  get_root_primitives,
  patch_field,
} from "../wasm-pkg/nrftw_wasm";
import { writable } from "svelte/store";
import {
  init as initZstd,
  createCCtx,
  freeCCtx,
  compressUsingDict,
} from "@bokuweb/zstd-wasm";
import { loadCatalog } from "./catalog";

export interface DocMeta {
  index: number;
  rootType: string;
}

export interface NodeEntry {
  key: string;
  path: string;
  type: string;
  isLeaf: boolean;
  value: string | null;
  childCount: number;
  guid?: string;
}

export interface PrimitiveField {
  path: string;
  type: string;
  value: string;
}

export type BootStatus =
  | { phase: "idle" }
  | { phase: "loading_wasm" }
  | { phase: "ready" }
  | { phase: "error"; message: string };

// ── Shared state stores ───────────────────────────────────────────────────────

export const catalogTarget = writable<string | null>(null);
export const editStash = writable<Record<number, Record<string, string>>>({});

// ── Module-level singletons ───────────────────────────────────────────────────

let dictBytes: Uint8Array = new Uint8Array(0);
let bridgePromise: Promise<void> | null = null;
const statusListeners: Array<(s: BootStatus) => void> = [];

function emitStatus(s: BootStatus) {
  for (const fn of statusListeners) fn(s);
}

export function onBootStatus(fn: (s: BootStatus) => void): () => void {
  statusListeners.push(fn);
  return () => {
    const i = statusListeners.indexOf(fn);
    if (i >= 0) statusListeners.splice(i, 1);
  };
}

// ── Zstd helpers (passed to Rust patch_field for compression) ─────────────────

function jsCompress(data: Uint8Array, dict: Uint8Array): Uint8Array {
  const cctx = createCCtx();
  try {
    return compressUsingDict(cctx, data, dict, 1);
  } finally {
    freeCCtx(cctx);
  }
}

// ── Boot ──────────────────────────────────────────────────────────────────────

async function _initBridge(): Promise<void> {
  try {
    emitStatus({ phase: "loading_wasm" });
    const base = import.meta.env.BASE_URL ?? "/";

    await Promise.all([init(), initZstd()]);

    const res = await fetch(`${base}cerimal_zstd.dict`);
    if (!res.ok) throw new Error(`fetch cerimal_zstd.dict: ${res.status}`);
    dictBytes = new Uint8Array(await res.arrayBuffer());

    // Load catalog in background — non-blocking, GUIDs resolve when ready
    loadCatalog().catch((err) =>
      console.warn("Catalog load failed (GUID resolution unavailable):", err),
    );

    emitStatus({ phase: "ready" });
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    emitStatus({ phase: "error", message });
    throw err;
  }
}

export function initBridge(): Promise<void> {
  if (!bridgePromise) {
    emitStatus({ phase: "loading_wasm" });
    bridgePromise = _initBridge();
  }
  return bridgePromise;
}

// ── High-level bridge functions ───────────────────────────────────────────────

export async function parseSaveFile(bytes: Uint8Array): Promise<DocMeta[]> {
  await bridgePromise;
  const json = parse_save(bytes, dictBytes) as string;
  return JSON.parse(json);
}

export async function getNodeChildren(
  docIdx: number,
  path: string,
): Promise<NodeEntry[]> {
  await bridgePromise;
  const json = get_node_children(docIdx, path) as string;
  return JSON.parse(json);
}

export async function getRootPrimitives(
  docIdx: number,
): Promise<PrimitiveField[]> {
  await bridgePromise;
  const json = get_root_primitives(docIdx) as string;
  return JSON.parse(json);
}

export async function patchField(
  docIdx: number,
  fieldName: string,
  value: string,
): Promise<Uint8Array> {
  await bridgePromise;
  return patch_field(
    docIdx,
    fieldName,
    value,
    jsCompress,
    dictBytes,
  ) as Uint8Array;
}

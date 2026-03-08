/* tslint:disable */
/* eslint-disable */

/**
 * Get children of the node at `path` in document `doc_idx`.
 * path="" means the root node.
 * Returns JSON: [{key, path, type, isLeaf, value, childCount, guid?}, ...]
 */
export function get_node_children(doc_idx: number, path: string): any;

/**
 * Get patchable primitive fields from the root composite of document `doc_idx`.
 * Returns JSON: [{path, type, value}, ...]
 */
export function get_root_primitives(doc_idx: number): any;

/**
 * Parse all CERIMAL documents in a save file.
 * dict_bytes: zstd dictionary bytes (pass empty slice for uncompressed saves).
 * Returns JSON: [{index, rootType}, ...]
 */
export function parse_save(data: Uint8Array, dict_bytes: Uint8Array): any;

/**
 * Patch a primitive field and return the full patched file bytes.
 * compress_fn: JS function (data: Uint8Array, dict: Uint8Array) -> Uint8Array
 */
export function patch_field(doc_idx: number, field_name: string, value_str: string, compress_fn: Function, dict_bytes: Uint8Array): Uint8Array;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly get_node_children: (a: number, b: number, c: number) => [number, number, number];
    readonly get_root_primitives: (a: number) => [number, number, number];
    readonly parse_save: (a: number, b: number, c: number, d: number) => [number, number, number];
    readonly patch_field: (a: number, b: number, c: number, d: number, e: number, f: any, g: number, h: number) => [number, number, number, number];
    readonly rust_zstd_wasm_shim_calloc: (a: number, b: number) => number;
    readonly rust_zstd_wasm_shim_free: (a: number) => void;
    readonly rust_zstd_wasm_shim_malloc: (a: number) => number;
    readonly rust_zstd_wasm_shim_memcmp: (a: number, b: number, c: number) => number;
    readonly rust_zstd_wasm_shim_memcpy: (a: number, b: number, c: number) => number;
    readonly rust_zstd_wasm_shim_memmove: (a: number, b: number, c: number) => number;
    readonly rust_zstd_wasm_shim_memset: (a: number, b: number, c: number) => number;
    readonly rust_zstd_wasm_shim_qsort: (a: number, b: number, c: number, d: number) => void;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;

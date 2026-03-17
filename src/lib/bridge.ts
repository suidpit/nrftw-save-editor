import init, {
    apply_changes,
    extract_customization,
    get_inventory_snapshot,
    get_node_children,
    get_root_primitives,
    parse_save,
    patch_field,
} from "../wasm-pkg/nrftw_wasm";
import { get, writable } from "svelte/store";
import { loadCatalog } from "./catalog";
import { isEquipmentAssetType } from "./inventory-assets";
import type { ItemEditorDraft, PendingItemChanges } from "./types";
import { isKnownUniqueItem } from "./unique-items";

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

export interface InventorySnapshotItem {
    index: number;
    itemPath: string;
    assetGuid: string;
    level: number;
    rarityNum: number;
    durability: number;
    stackCount: number;
    slotNum: number;
    runeGuids: string[];
    enchantmentGuids: string[];
    traitGuid: string;
}

export type BootStatus =
    | { phase: "idle" }
    | { phase: "loading_wasm" }
    | { phase: "ready" }
    | { phase: "error"; message: string };

export const catalogTarget = writable<string | null>(null);
export const editStash = writable<Record<number, Record<string, string>>>({});
export const pendingItems = writable<PendingItemChanges>({
    edits: {},
    creates: [],
    deletes: [],
});

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

async function _initBridge(): Promise<void> {
    try {
        emitStatus({ phase: "loading_wasm" });
        const base = import.meta.env.BASE_URL ?? "/";

        await init();

        const res = await fetch(`${base}cerimal_zstd.dict`);
        if (!res.ok) throw new Error(`fetch cerimal_zstd.dict: ${res.status}`);
        dictBytes = new Uint8Array(await res.arrayBuffer());

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

export async function parseSaveFile(bytes: Uint8Array): Promise<DocMeta[]> {
    await bridgePromise;
    editStash.set({});
    pendingItems.set({ edits: {}, creates: [], deletes: [] });
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

export async function getInventorySnapshot(
    docIdx: number,
): Promise<InventorySnapshotItem[]> {
    await bridgePromise;
    const json = get_inventory_snapshot(docIdx) as string;
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
        dictBytes,
    ) as Uint8Array;
}

export function stageItemChange(draft: ItemEditorDraft) {
    pendingItems.update((state) => {
        if (draft.isNew) {
            const creates = [...state.creates];
            const idx = creates.findIndex((item) => item.draftId === draft.draftId);
            if (idx >= 0) creates[idx] = draft;
            else creates.push(draft);
            return { ...state, creates };
        }

        if (!draft.itemPath) return state;
        return {
            ...state,
            edits: {
                ...state.edits,
                [draft.itemPath]: draft,
            },
        };
    });
}

export function parseInventoryIndex(itemPath: string): number | null {
    const match = /^Inventory\[(\d+)\]$/.exec(itemPath);
    return match ? Number(match[1]) : null;
}

function toInventoryPath(index: number): string {
    return `Inventory[${index}]`;
}

function rebasePathAfterDeletion(itemPath: string, deletedPath: string): string | null {
    const itemIndex = parseInventoryIndex(itemPath);
    const deletedIndex = parseInventoryIndex(deletedPath);
    if (itemIndex === null || deletedIndex === null) return itemPath;
    if (itemIndex === deletedIndex) return null;
    if (itemIndex < deletedIndex) return itemPath;
    return toInventoryPath(itemIndex - 1);
}

export function deleteItemChange(draft: ItemEditorDraft) {
    pendingItems.update((state) => {
        if (draft.isNew) {
            return {
                ...state,
                creates: state.creates.filter((item) => item.draftId !== draft.draftId),
            };
        }

        if (!draft.itemPath) return state;

        const edits: Record<string, ItemEditorDraft> = {};
        for (const [path, editDraft] of Object.entries(state.edits)) {
            const rebasedPath = rebasePathAfterDeletion(path, draft.itemPath);
            if (!rebasedPath) continue;
            edits[rebasedPath] = {
                ...editDraft,
                itemPath: rebasedPath,
            };
        }

        return {
            edits,
            creates: state.creates,
            deletes: [
                ...state.deletes,
                {
                    docIdx: draft.docIdx,
                    itemPath: draft.itemPath,
                },
            ],
        };
    });
}

/// Parse source save bytes (without touching loaded state) and stage all
/// Customization fields as pending stat edits on the CharacterMetadata doc.
/// Returns the number of fields imported.
export async function importAppearanceFromBytes(
    sourceBytes: Uint8Array,
    metaDocIdx: number,
): Promise<number> {
    await bridgePromise;
    const json = extract_customization(sourceBytes, dictBytes) as string;
    const fields: Record<string, string> = JSON.parse(json);
    const entries = Object.entries(fields);
    if (entries.length === 0) return 0;

    editStash.update((stash) => ({
        ...stash,
        [metaDocIdx]: { ...(stash[metaDocIdx] ?? {}), ...fields },
    }));

    return entries.length;
}

export function resetPendingChanges() {
    editStash.set({});
    pendingItems.set({ edits: {}, creates: [], deletes: [] });
}

export async function applyPendingChanges(): Promise<Uint8Array> {
    await bridgePromise;

    const currentItems = get(pendingItems);
    const currentStats = get(editStash);

    const statEdits = Object.entries(currentStats).flatMap(([docIdx, fields]) =>
        Object.entries(fields).map(([path, value]) => ({
            doc_idx: Number(docIdx),
            path,
            value,
        })),
    );

    const toPayload = (draft: ItemEditorDraft) => ({
        ...(() => {
            const isEquipmentItem = isEquipmentAssetType(draft.assetType);
            return {
                doc_idx: draft.docIdx,
                item_path: draft.itemPath,
                asset_guid: draft.assetGuid,
                level: draft.level,
                rarity:
                    draft.rarityNum === 3 && !isKnownUniqueItem(draft.assetGuid)
                        ? 2
                        : draft.rarityNum,
                durability: isEquipmentItem ? draft.durability : 0,
                stack_count: draft.stackCount,
                rune_guids: isEquipmentItem ? draft.runeGuids : [],
                enchantment_guids: isEquipmentItem ? draft.enchantmentGuids : [],
            };
        })(),
    });

    const payload = {
        stat_edits: statEdits,
        item_deletes: currentItems.deletes.map((draft) => ({
            doc_idx: draft.docIdx,
            item_path: draft.itemPath,
        })),
        item_edits: Object.values(currentItems.edits).map(toPayload),
        item_creates: currentItems.creates.map(toPayload),
    };

    return apply_changes(JSON.stringify(payload), dictBytes) as Uint8Array;
}

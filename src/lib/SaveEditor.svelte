<script lang="ts">
    import { writable } from "svelte/store";
    import { setContext } from "svelte";
    import {
        editStash,
        getNodeChildren,
        patchField,
        type DocMeta,
        type NodeEntry,
    } from "./bridge";
    import TreeNode from "./TreeNode.svelte";

    export let docs: DocMeta[];
    export let fileName: string = "save.cerimal";
    export let onGuidClick: (guid: string) => void = () => {};

    let selectedDoc = 0;
    let prevDoc = -1;
    let error = "";
    let applying = false;

    // Tree state — provided via context to TreeNode
    const cache = writable<Record<string, NodeEntry[] | null | undefined>>({});
    const expanded = writable<Set<string>>(new Set());
    const pendingEdits = writable<Record<string, string>>({});

    async function loadChildren(path: string) {
        cache.update((c) => ({ ...c, [path]: null }));
        const children = await getNodeChildren(selectedDoc, path);
        cache.update((c) => ({ ...c, [path]: children }));
    }

    function onEditValue(path: string, value: string) {
        console.log(`Editing value ${path} to ${value}`);
        pendingEdits.update((e) => {
            const next = { ...e };
            if (value === "") {
                delete next[path];
            } else {
                next[path] = value;
            }
            return next;
        });
    }

    setContext("tree", {
        cache,
        expanded,
        loadChildren,
        onGuidClick,
        edits: pendingEdits,
        onEditValue,
    });

    $: if (selectedDoc !== undefined && prevDoc === selectedDoc) {
        editStash.update((stash) => ({
            ...stash,
            [selectedDoc]: $pendingEdits,
        }));
    }

    $: {
        selectedDoc;
        if (prevDoc >= 0) {
            const current = {
                ...(typeof $pendingEdits === "object" ? $pendingEdits : {}),
            };
            editStash.update((stash) => {
                const next = { ...stash };
                if (Object.keys(current).length > 0) {
                    next[prevDoc] = current;
                } else {
                    delete next[prevDoc];
                }
                return next;
            });
        }
        pendingEdits.set($editStash[selectedDoc] ?? {});
        prevDoc = selectedDoc;
        cache.set({});
        expanded.set(new Set());
        loadChildren("");
    }

    $: rootChildren = $cache[""];

    export async function applyPatches() {
        const patches = Object.entries($pendingEdits);
        if (patches.length === 0) return;
        applying = true;
        error = "";
        try {
            let result: Uint8Array | null = null;
            for (const [fieldPath, rawVal] of patches) {
                result = await patchField(selectedDoc, fieldPath, rawVal);
            }
            if (result) {
                downloadBytes(result, fileName + ".patched");
            }
        } catch (e) {
            error = String(e);
        } finally {
            applying = false;
        }
    }

    function downloadBytes(bytes: Uint8Array, name: string) {
        const copy = new Uint8Array(bytes);
        const blob = new Blob([copy], {
            type: "application/octet-stream",
        });
        const url = URL.createObjectURL(blob);
        const a = document.createElement("a");
        a.href = url;
        a.download = name;
        a.click();
        URL.revokeObjectURL(url);
    }

    export function resetEdits() {
        pendingEdits.set({});
    }
</script>

<div class="editor">
    <div class="toolbar">
        {#if docs.length > 1}
            <div class="doc-tabs">
                {#each docs as doc}
                    <button
                        class="doc-tab"
                        class:active={selectedDoc === doc.index}
                        on:click={() => (selectedDoc = doc.index)}
                    >
                        Doc {doc.index}: {doc.rootType}
                    </button>
                {/each}
            </div>
        {/if}
    </div>

    {#if error}
        <div class="error">{error}</div>
    {/if}

    <div class="section tree-section">
        <div class="tree">
            {#if rootChildren === undefined || rootChildren === null}
                <div class="loading">Loading tree…</div>
            {:else if rootChildren.length === 0}
                <div class="empty">No fields found.</div>
            {:else}
                {#each rootChildren as node (node.path + "|" + node.key)}
                    <TreeNode {node} depth={0} />
                {/each}
            {/if}
        </div>
    </div>
</div>

<style>
    .editor {
        height: 100%;
        display: flex;
        flex-direction: column;
        overflow: hidden;
    }

    /* Toolbar */
    .toolbar {
        display: flex;
        align-items: center;
        gap: 8px;
        padding: 6px 8px;
        border-bottom: 1px solid #2e2e2e;
        flex-shrink: 0;
        min-height: 36px;
    }
    .doc-tabs {
        display: flex;
        gap: 4px;
    }
    .doc-tab {
        padding: 3px 10px;
        font-size: 0.8em;
        border-radius: 4px;
        border: 1px solid #444;
        background: #1a1a1a;
        color: #aaa;
        cursor: pointer;
    }
    .doc-tab.active {
        background: #2a2a2a;
        color: #fff;
        border-color: #555;
    }

    .error {
        margin: 8px 16px 0;
        padding: 8px 12px;
        background: rgba(255, 80, 80, 0.1);
        border: 1px solid rgba(255, 80, 80, 0.3);
        border-radius: 6px;
        color: #f88;
        font-size: 0.85em;
        flex-shrink: 0;
    }

    /* Tree section */
    .section {
        padding: 12px 16px;
    }
    .tree-section {
        flex: 1;
        overflow: hidden;
        display: flex;
        flex-direction: column;
    }
    .tree {
        flex: 1;
        overflow: auto;
        font-family: "Cascadia Code", "Fira Mono", monospace;
        font-size: 0.82em;
    }
    .loading,
    .empty {
        color: #555;
        font-style: italic;
        padding: 8px;
    }
</style>

<script lang="ts">
    import { writable } from "svelte/store";
    import { setContext } from "svelte";
    import {
        getNodeChildren,
        type DocMeta,
        type NodeEntry,
    } from "./bridge";
    import TreeNode from "./TreeNode.svelte";

    export let docs: DocMeta[];
    export let onGuidClick: (guid: string) => void = () => {};

    let selectedDoc = 0;

    const cache = writable<Record<string, NodeEntry[] | null | undefined>>({});
    const expanded = writable<Set<string>>(new Set());
    // Read-only: edits store is always empty, onEditValue is a no-op
    const noEdits = writable<Record<string, string>>({});

    async function loadChildren(path: string) {
        cache.update(c => ({ ...c, [path]: null }));
        const children = await getNodeChildren(selectedDoc, path);
        cache.update(c => ({ ...c, [path]: children }));
    }

    setContext("tree", {
        cache,
        expanded,
        loadChildren,
        onGuidClick,
        edits: noEdits,
        onEditValue: () => {},
        readonly: true,
    });

    // On doc switch: reset view
    $: {
        selectedDoc;
        cache.set({});
        expanded.set(new Set());
        loadChildren("");
    }

    $: rootChildren = $cache[""];

    // No-ops kept for interface compatibility with MainLayout
    export function resetEdits() {}
    export async function applyPatches(): Promise<Uint8Array | null> { return null; }
</script>

<div class="inspector">
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

    <div class="tree-section">
        <div class="tree">
            {#if rootChildren === undefined || rootChildren === null}
                <div class="tree-msg">Loading tree…</div>
            {:else if rootChildren.length === 0}
                <div class="tree-msg">No fields found.</div>
            {:else}
                {#each rootChildren as node (node.path + "|" + node.key)}
                    <TreeNode {node} depth={0} />
                {/each}
            {/if}
        </div>
    </div>
</div>

<style>
    .inspector {
        height: 100%;
        display: flex;
        flex-direction: column;
        overflow: hidden;
    }

    .doc-tabs {
        display: flex;
        gap: 4px;
        padding: 6px 8px;
        border-bottom: 1px solid var(--border-color, #4a3520);
        flex-shrink: 0;
    }

    .doc-tab {
        padding: 3px 10px;
        font-size: 0.78em;
        border-radius: 4px;
        border: 1px solid var(--border-color, #4a3520);
        background: var(--bg-card, #2d2010);
        color: var(--text-secondary, #a89070);
        cursor: pointer;
        font-family: inherit;
        transition: background 0.15s;
    }

    .doc-tab.active {
        background: var(--bg-panel, #231a0e);
        color: var(--text-primary, #e8d5a3);
        border-color: var(--accent-gold, #c8a050);
    }

    .tree-section {
        flex: 1;
        overflow: hidden;
        display: flex;
        flex-direction: column;
        padding: 8px 12px;
    }

    .tree {
        flex: 1;
        overflow: auto;
        font-family: var(--font-mono);
        font-size: 0.82em;
    }

    .tree-msg {
        color: var(--text-secondary, #a89070);
        font-style: italic;
        padding: 8px;
    }
</style>

<script lang="ts">
    import type { NodeEntry } from "./bridge";
    import { getContext } from "svelte";
    import type { Writable } from "svelte/store";
    import { getAssetByGuid } from "./catalog";

    export let node: NodeEntry;
    export let depth: number = 0;

    interface TreeCtx {
        cache: Writable<Record<string, NodeEntry[] | null | undefined>>;
        expanded: Writable<Set<string>>;
        loadChildren: (path: string) => void;
        onGuidClick: (guid: string) => void;
        edits: Writable<Record<string, string>>;
        onEditValue: (path: string, value: string) => void;
        readonly?: boolean;
    }

    const EDITABLE_TYPES = new Set([
        "BOOL",
        "BYTE",
        "SBYTE",
        "SHORT",
        "USHORT",
        "CHAR",
        "INT",
        "UINT",
        "FLOAT",
        "LONG",
        "ULONG",
        "DOUBLE",
        "FP",
        "LFP",
        "NINT",
        "NUINT",
    ]);

    const { cache, expanded, loadChildren, onGuidClick, edits, onEditValue, readonly } =
        getContext<TreeCtx>("tree");

    $: isExpanded = $expanded.has(node.path);
    $: children = node.path ? $cache[node.path] : $cache[""];
    $: loadState =
        node.path in $cache || "" in $cache
            ? children === null
                ? "loading"
                : "loaded"
            : "idle";

    $: isEditable = !readonly && node.isLeaf && EDITABLE_TYPES.has(node.type);
    $: pendingValue = $edits[node.path];
    $: hasEdit = pendingValue !== undefined && pendingValue !== "";
    $: displayValue = hasEdit ? pendingValue : node.value;

    let editing = false;
    let editValue = "";

    function startEdit() {
        if (!isEditable) return;
        editValue = hasEdit ? pendingValue : (node.value ?? "");
        editing = true;
    }

    function commitEdit() {
        editing = false;
        const trimmed = editValue.trim();
        if (trimmed !== "" && trimmed !== node.value) {
            onEditValue(node.path, trimmed);
        } else if (trimmed === node.value || trimmed === "") {
            // Clear edit if reverted to original or empty
            onEditValue(node.path, "");
        }
    }

    function cancelEdit() {
        editing = false;
    }

    function onKeydown(e: KeyboardEvent) {
        if (e.key === "Enter") commitEdit();
        else if (e.key === "Escape") cancelEdit();
    }

    function toggle() {
        if (node.isLeaf) return;
        expanded.update((s) => {
            const next = new Set(s);
            if (next.has(node.path)) {
                next.delete(node.path);
            } else {
                next.add(node.path);
                if (!($cache[node.path] !== undefined)) {
                    loadChildren(node.path);
                }
            }
            return next;
        });
    }

    const INDENT = 18; // px per depth level
</script>

<div class="node" style="padding-left: {depth * INDENT}px">
    <span
        class="expander"
        on:click={toggle}
        role="button"
        tabindex="0"
        on:keydown={(e) => e.key === "Enter" && toggle()}
    >
        {#if !node.isLeaf}
            {isExpanded ? "▾" : "▸"}
        {:else}
            <span class="leaf-bullet">·</span>
        {/if}
    </span>

    <span class="key">{node.key}</span>

    {#if node.type}
        <span class="type-badge">{node.type}</span>
    {/if}

    {#if node.value !== null && node.value !== undefined}
        {#if node.type === "ASSETGUID" && node.guid != null && node.guid != "0"}
            <button
                class="guid-link"
                data-guid={node.guid}
                on:click={() => onGuidClick(node.guid!)}
                >{getAssetByGuid(node.value)?.name ?? node.guid}</button
            >
        {:else if isEditable}
            {#if editing}
                <input
                    class="inline-edit"
                    type="text"
                    bind:value={editValue}
                    on:keydown={onKeydown}
                    on:blur={commitEdit}
                    placeholder={node.value ?? ""}
                />
            {:else}
                <span
                    class="value editable"
                    class:edited={hasEdit}
                    on:click={startEdit}
                    role="button"
                    tabindex="0"
                    on:keydown={(e) => e.key === "Enter" && startEdit()}
                    title="Click to edit">{displayValue}</span
                >
            {/if}
        {:else}
            <span class="value">{node.value}</span>
        {/if}
    {/if}

    {#if !node.isLeaf && node.childCount !== undefined && node.childCount > 0 && !isExpanded}
        <span class="count-hint">[{node.childCount}]</span>
    {/if}
</div>

{#if isExpanded && !node.isLeaf}
    {#if $cache[node.path] === null}
        <div class="loading-row" style="padding-left: {(depth + 1) * INDENT}px">
            loading…
        </div>
    {:else if $cache[node.path]}
        {#each $cache[node.path] as child (child.path + "|" + child.key)}
            <svelte:self node={child} depth={depth + 1} />
        {/each}
    {/if}
{/if}

<style>
    .node {
        display: flex;
        align-items: baseline;
        gap: 6px;
        line-height: 1.6;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }
    .node:hover {
        background: rgba(255, 255, 255, 0.04);
    }
    .expander {
        width: 14px;
        flex-shrink: 0;
        cursor: pointer;
        color: #777;
        user-select: none;
    }
    .leaf-bullet {
        color: #444;
    }
    .key {
        color: #9cdcfe;
        flex-shrink: 0;
    }
    .type-badge {
        color: #569cd6;
        font-size: 0.8em;
        opacity: 0.7;
        flex-shrink: 0;
    }
    .value {
        color: #ce9178;
        flex: 1;
        overflow: hidden;
        text-overflow: ellipsis;
    }
    .value.editable {
        cursor: pointer;
        text-decoration: underline dashed;
        text-decoration-color: transparent;
        text-underline-offset: 2px;
    }
    .value.editable:hover {
        text-decoration-color: #ce9178;
    }
    .value.edited {
        color: #646cff;
    }
    .inline-edit {
        width: 140px;
        background: #1a1a1a;
        border: 1px solid #646cff;
        border-radius: 3px;
        padding: 1px 4px;
        color: #fff;
        font-family: inherit;
        font-size: 1em;
        outline: none;
    }
    .guid-link {
        color: #ce9178;
        text-decoration: underline dotted;
        cursor: pointer;
        background: none;
        border: none;
        padding: 0;
        font: inherit;
        flex: 1;
        overflow: hidden;
        text-overflow: ellipsis;
        text-align: left;
    }
    .guid-link:hover {
        color: #646cff;
    }
    .count-hint {
        color: #555;
        font-size: 0.8em;
    }
    .loading-row {
        color: #555;
        font-style: italic;
        line-height: 1.6;
    }
</style>

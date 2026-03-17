<script lang="ts">
    import { createEventDispatcher } from "svelte";
    import {
        applyPendingChanges,
        catalogTarget,
        editStash,
        pendingItems,
        resetPendingChanges,
        type DocMeta,
    } from "./bridge";
    import Editor from "./Editor.svelte";
    import Inspector from "./Inspector.svelte";
    import CatalogSearch from "./CatalogSearch.svelte";

    export let docs: DocMeta[];
    export let fileName: string;

    const dispatch = createEventDispatcher<{ resetfile: void }>();

    type Tab = "editor" | "inspector" | "catalog";
    let activeTab: Tab = "editor";

    const tabDefs: [Tab, string][] = [
        ["editor", "Editor"],
        ["inspector", "Inspector"],
        ["catalog", "Catalog"],
    ];

    let inspectorRef: Inspector;
    let applying = false;
    let downloadError = "";

    $: statEditCount = Object.values($editStash).reduce(
        (sum, edits) => sum + Object.keys(edits).length,
        0,
    );
    $: itemEditCount =
        Object.keys($pendingItems.edits).length +
        $pendingItems.creates.length +
        $pendingItems.deletes.length;
    $: editCount = statEditCount + itemEditCount;

    function handleGuidClick(guid: string) {
        catalogTarget.set(guid);
        activeTab = "catalog";
    }

    async function downloadPatched() {
        if (editCount === 0) return;
        applying = true;
        downloadError = "";
        try {
            const bytes = await applyPendingChanges();
            downloadBlob(bytes, fileName);
            resetEdits();
        } catch (e) {
            downloadError = String(e);
        } finally {
            applying = false;
        }
    }

    function resetEdits() {
        resetPendingChanges();
        inspectorRef?.resetEdits();
    }

    function downloadBlob(bytes: Uint8Array, name: string) {
        const blob = new Blob([new Uint8Array(bytes)], {
            type: "application/octet-stream",
        });
        const url = URL.createObjectURL(blob);
        const a = document.createElement("a");
        a.href = url;
        a.download = name;
        a.click();
        URL.revokeObjectURL(url);
    }
</script>

<div class="main-layout">
    <header class="layout-header">
        <div class="title-area">
            <span class="save-name">{fileName}</span>
            <button
                class="load-another-btn"
                on:click={() => dispatch("resetfile")}
            >
                Load another save...
            </button>
        </div>

        <nav class="tab-nav">
            {#each tabDefs as [tab, label]}
                <button
                    class="tab"
                    class:active={activeTab === tab}
                    on:click={() => (activeTab = tab)}
                >
                    {label}
                </button>
            {/each}
        </nav>

        <div class="toolbar">
            {#if editCount > 0}
                <span class="edit-badge"
                    >{editCount} edit{editCount !== 1 ? "s" : ""}</span
                >
                <button class="reset-btn" on:click={resetEdits}>Reset</button>
                <button
                    class="download-btn"
                    disabled={applying}
                    on:click={downloadPatched}
                >
                    {applying ? "Saving…" : "Download patched"}
                </button>
            {/if}
        </div>
    </header>

    {#if downloadError}
        <div class="download-error">{downloadError}</div>
    {/if}

    <main class="layout-main">
        {#if activeTab === "editor"}
            <Editor {docs} />
        {:else if activeTab === "inspector"}
            <Inspector
                bind:this={inspectorRef}
                {docs}
                onGuidClick={handleGuidClick}
            />
        {:else if activeTab === "catalog"}
            <CatalogSearch />
        {/if}
    </main>
</div>

<style>
    .main-layout {
        height: 100vh;
        display: flex;
        flex-direction: column;
        overflow: hidden;
        background: var(--bg-primary, #1a1209);
    }

    .layout-header {
        display: flex;
        align-items: center;
        gap: 12px;
        padding: 0 16px;
        height: 44px;
        background: var(--bg-panel, #231a0e);
        border-bottom: 1px solid var(--border-color, #4a3520);
        flex-shrink: 0;
    }

    .title-area {
        flex-shrink: 0;
        display: flex;
        align-items: center;
        gap: 8px;
    }

    .save-name {
        font-size: 0.78em;
        color: var(--text-secondary, #a89070);
        font-family: var(--font-mono);
        white-space: nowrap;
        max-width: 180px;
        overflow: hidden;
        text-overflow: ellipsis;
        display: block;
    }

    .load-another-btn {
        font-size: 0.76em;
        padding: 3px 10px;
        border-radius: 4px;
        border: 1px solid var(--border-color, #4a3520);
        color: var(--text-secondary, #a89070);
        background: transparent;
        font-family: inherit;
        white-space: nowrap;
        transition:
            color 0.15s,
            border-color 0.15s,
            background 0.15s;
    }

    .load-another-btn:hover {
        color: var(--text-primary, #e8d5a3);
        border-color: var(--accent-gold, #c8a050);
        background: rgba(200, 160, 80, 0.08);
    }

    .tab-nav {
        display: flex;
        gap: 2px;
        margin: 0 auto;
    }

    .tab {
        padding: 5px 16px;
        border-radius: 5px;
        font-size: 0.84em;
        background: transparent;
        border: 1px solid transparent;
        color: var(--text-secondary, #a89070);
        cursor: pointer;
        font-family: inherit;
        transition:
            background 0.15s,
            color 0.15s,
            border-color 0.15s;
    }

    .tab:hover:not(.active) {
        background: var(--bg-card, #2d2010);
        color: var(--text-primary, #e8d5a3);
    }

    .tab.active {
        background: var(--bg-card, #2d2010);
        border-color: var(--accent-gold, #c8a050);
        color: var(--accent-gold, #c8a050);
    }

    .toolbar {
        display: flex;
        align-items: center;
        gap: 8px;
        flex-shrink: 0;
    }

    .edit-badge {
        font-size: 0.75em;
        color: var(--accent-gold, #c8a050);
        background: rgba(200, 160, 80, 0.12);
        padding: 2px 8px;
        border-radius: 10px;
        border: 1px solid rgba(200, 160, 80, 0.3);
        white-space: nowrap;
    }

    .reset-btn {
        font-size: 0.78em;
        padding: 3px 10px;
        border-radius: 4px;
        border: 1px solid var(--border-color, #4a3520);
        color: var(--text-secondary, #a89070);
        background: transparent;
        font-family: inherit;
        transition:
            color 0.15s,
            border-color 0.15s;
    }

    .reset-btn:hover {
        color: var(--text-primary, #e8d5a3);
        border-color: var(--accent-gold, #c8a050);
    }

    .download-btn {
        font-size: 0.8em;
        padding: 5px 12px;
        border-radius: 4px;
        border: 1px solid var(--accent-gold, #c8a050);
        background: rgba(200, 160, 80, 0.12);
        color: var(--accent-gold, #c8a050);
        font-family: inherit;
    }

    .download-btn:hover:not(:disabled) {
        background: rgba(200, 160, 80, 0.2);
    }

    .download-btn:disabled {
        opacity: 0.6;
        cursor: wait;
    }

    .download-error {
        padding: 8px 16px;
        font-size: 0.82em;
        color: #ffb0b0;
        background: rgba(128, 0, 0, 0.18);
        border-bottom: 1px solid rgba(255, 120, 120, 0.2);
    }

    .layout-main {
        flex: 1;
        overflow: hidden;
    }
</style>

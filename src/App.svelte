<script lang="ts">
    import { onMount } from "svelte";
    import {
        initBridge,
        onBootStatus,
        parseSaveFile,
        catalogTarget,
        editStash,
        type DocMeta,
        type BootStatus,
    } from "./lib/bridge";
    import SaveEditor from "./lib/SaveEditor.svelte";
    import CatalogSearch from "./lib/CatalogSearch.svelte";

    type Tab = "editor" | "catalog";
    let activeTab: Tab = "editor";
    let editorRef: SaveEditor;

    $: editCount = Object.values($editStash).reduce(
        (sum, edits) => sum + Object.keys(edits).length,
        0,
    );
    $: console.log("Recomputed editCount:", editCount);

    let bootStatus: BootStatus = { phase: "idle" };
    let docs: DocMeta[] = [];
    let fileName = "";
    let parseError = "";
    let parsing = false;

    const phaseLabel: Record<string, string> = {
        idle: "Idle",
        loading_wasm: "Loading WASM…",
        ready: "Ready",
        error: "Error",
    };

    onMount(() => {
        const unsub = onBootStatus((s) => (bootStatus = s));
        initBridge();
        return unsub;
    });

    async function handleFile(file: File) {
        if (bootStatus.phase !== "ready") {
            parseError = "WASM not ready yet — wait for boot to finish.";
            return;
        }
        parsing = true;
        parseError = "";
        fileName = file.name;
        try {
            const buf = await file.arrayBuffer();
            docs = await parseSaveFile(new Uint8Array(buf));
        } catch (e) {
            parseError = String(e);
            docs = [];
        } finally {
            parsing = false;
        }
    }

    function onFileInput(e: Event) {
        const input = e.target as HTMLInputElement;
        if (input.files?.[0]) handleFile(input.files[0]);
    }

    function onDrop(e: DragEvent) {
        e.preventDefault();
        const file = e.dataTransfer?.files?.[0];
        if (file) handleFile(file);
    }

    function onDragOver(e: DragEvent) {
        e.preventDefault();
    }

    function handleGuidClick(guid: string) {
        catalogTarget.set(guid);
        activeTab = "catalog";
    }
</script>

<div class="app">
    <header class="app-header">
        <h1 class="app-title">NRFTW Save Tool</h1>

        <div
            class="boot-status"
            class:ready={bootStatus.phase === "ready"}
            class:error={bootStatus.phase === "error"}
        >
            {#if bootStatus.phase === "error"}
                ✕ {bootStatus.message}
            {:else}
                {phaseLabel[bootStatus.phase] ?? bootStatus.phase}
            {/if}
        </div>

        <nav class="tabs">
            {#each ["editor", "catalog"] as Tab[] as tab}
                <button
                    class="tab"
                    class:active={activeTab === tab}
                    on:click={() => (activeTab = tab)}
                >
                    {tab.charAt(0).toUpperCase() + tab.slice(1)}
                </button>
            {/each}
        </nav>
    </header>

    {#if activeTab !== "catalog"}
        <div
            class="drop-zone"
            class:has-file={docs.length > 0}
            on:drop={onDrop}
            on:dragover={onDragOver}
            role="button"
            tabindex="0"
            aria-label="Drop zone"
        >
            {#if docs.length > 0}
                <span class="file-name">📄 {fileName}</span>
                <label class="change-btn">
                    Change file
                    <input
                        type="file"
                        accept=".cerimal"
                        on:change={onFileInput}
                        style="display:none"
                    />
                </label>

                {#if editCount > 0}
                    <span class="edit-badge"
                        >{editCount} edit{editCount !== 1 ? "s" : ""}</span
                    >
                    <button
                        class="reset-btn"
                        on:click={() => editorRef?.resetEdits()}>Reset</button
                    >
                    <button
                        class="download-btn"
                        on:click={() => editorRef?.applyPatches()}
                        >Download patched</button
                    >
                {/if}
            {:else}
                <div class="drop-hint">
                    {#if parsing}
                        Parsing…
                    {:else}
                        <label class="file-pick">
                            Drop a <code>.cerimal</code> file here or
                            <span class="link">click to browse</span>
                            <input
                                type="file"
                                accept=".cerimal"
                                on:change={onFileInput}
                                style="display:none"
                            />
                        </label>
                    {/if}
                </div>
            {/if}

            {#if parseError}
                <div class="parse-error">{parseError}</div>
            {/if}
        </div>
    {/if}

    <main class="main-content">
        {#if activeTab === "editor"}
            {#if docs.length > 0}
                <SaveEditor
                    bind:this={editorRef}
                    {docs}
                    {fileName}
                    onGuidClick={handleGuidClick}
                />
            {:else}
                <div class="empty-state">
                    Load a <code>.cerimal</code> save file to edit fields.
                </div>
            {/if}
        {:else if activeTab === "catalog"}
            <CatalogSearch />
        {/if}
    </main>
</div>

<style>
    .app {
        height: 100vh;
        display: flex;
        flex-direction: column;
        overflow: hidden;
    }
    .app-header {
        display: flex;
        align-items: center;
        gap: 12px;
        padding: 8px 16px;
        background: #1a1a1a;
        border-bottom: 1px solid #333;
        flex-shrink: 0;
    }
    .app-title {
        font-size: 1em;
        font-weight: 700;
        margin: 0;
        color: #ddd;
        white-space: nowrap;
    }
    .boot-status {
        font-size: 0.75em;
        color: #666;
        white-space: nowrap;
        transition: color 0.3s;
    }
    .boot-status.ready {
        color: #4caf50;
    }
    .boot-status.error {
        color: #f44;
    }
    .tabs {
        margin-left: auto;
        display: flex;
        gap: 2px;
    }
    .tab {
        padding: 5px 14px;
        border-radius: 6px;
        font-size: 0.85em;
        background: transparent;
        border: 1px solid transparent;
        color: #888;
    }
    .tab.active {
        background: #646cff22;
        border-color: #646cff66;
        color: #ccc;
    }
    .tab:hover:not(.active) {
        background: #2a2a2a;
        color: #ccc;
    }
    .drop-zone {
        display: flex;
        align-items: center;
        gap: 12px;
        padding: 10px 16px;
        background: #1e1e1e;
        border-bottom: 1px solid #2e2e2e;
        flex-shrink: 0;
        min-height: 44px;
    }
    .drop-zone.has-file {
        background: #1e2318;
        border-bottom-color: #2e3928;
    }
    .drop-hint {
        font-size: 0.85em;
        color: #777;
    }
    .file-pick {
        cursor: pointer;
    }
    .link {
        color: #646cff;
        text-decoration: underline;
        cursor: pointer;
    }
    .file-name {
        font-size: 0.85em;
        color: #aaa;
        font-family: monospace;
    }
    .change-btn {
        font-size: 0.78em;
        padding: 3px 8px;
        border-radius: 4px;
        border: 1px solid #444;
        cursor: pointer;
        color: #888;
        background: transparent;
    }
    .edit-badge {
        font-size: 0.78em;
        color: #646cff;
        background: #646cff1a;
        padding: 2px 8px;
        border-radius: 10px;
        white-space: nowrap;
    }
    .reset-btn {
        font-size: 0.78em;
        padding: 3px 8px;
        border-radius: 4px;
        border: 1px solid #444;
        cursor: pointer;
        color: #888;
        background: transparent;
    }
    .reset-btn:hover {
        color: #ccc;
        border-color: #666;
    }
    .download-btn {
        font-size: 0.78em;
        padding: 3px 10px;
        border-radius: 4px;
        border: none;
        cursor: pointer;
        color: #fff;
        background: #646cff;
        font-weight: 600;
    }
    .download-btn:hover {
        background: #5a62ef;
    }
    .parse-error {
        flex: 1;
        font-size: 0.8em;
        color: #f88;
        padding: 4px 8px;
        background: rgba(255, 60, 60, 0.08);
        border-radius: 4px;
    }
    .main-content {
        flex: 1;
        overflow: hidden;
    }
    .empty-state {
        padding: 32px;
        color: #555;
        text-align: center;
        font-size: 0.9em;
    }
    code {
        background: #2a2a2a;
        padding: 1px 4px;
        border-radius: 3px;
        font-family: monospace;
    }
</style>

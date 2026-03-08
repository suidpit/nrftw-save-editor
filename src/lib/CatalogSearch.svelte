<script lang="ts">
    import { catalogTarget } from "./bridge";
    import {
        loadCatalog,
        searchCatalog,
        getScriptTypes,
        getAssetByGuid,
        type CatalogRow,
        type AssetDetail,
    } from "./catalog";

    let query = "";
    let selectedType = "";
    let scriptTypes: string[] = [];
    let results: CatalogRow[] = [];
    let catalogLoading = false;
    let catalogReady = false;
    let searching = false;
    let error = "";
    let debounceTimer: ReturnType<typeof setTimeout>;

    let selectedAsset: AssetDetail | null = null;
    let assetLoading = false;

    let _catalogPromise: Promise<void> | null = null;

    function ensureCatalog(): Promise<void> {
        console.log(
            "[catalog] ensureCatalog called, catalogReady=",
            catalogReady,
            "catalogLoading=",
            catalogLoading,
            "hasPromise=",
            !!_catalogPromise,
        );
        if (!_catalogPromise) {
            _catalogPromise = (async () => {
                if (catalogReady) return;
                catalogLoading = true;
                error = "";
                try {
                    console.log("[catalog] calling loadCatalog…");
                    await loadCatalog();
                    console.log("[catalog] loadCatalog done");
                    catalogReady = true;
                    scriptTypes = getScriptTypes();
                    console.log(
                        "[catalog] ready, scriptTypes=",
                        scriptTypes.length,
                    );
                } catch (e) {
                    console.error("[catalog] ensureCatalog error:", e);
                    error = `Failed to load catalog: ${e}`;
                } finally {
                    catalogLoading = false;
                }
            })();
        }
        return _catalogPromise;
    }

    // Lazy load catalog on component mount
    ensureCatalog();

    async function openDetail(guid: string) {
        console.log(
            "[catalog] openDetail guid=",
            guid,
            "catalogReady=",
            catalogReady,
        );
        if (!catalogReady) {
            console.log("[catalog] waiting for ensureCatalog…");
            await ensureCatalog();
            console.log(
                "[catalog] ensureCatalog resolved, catalogReady=",
                catalogReady,
            );
        }
        selectedAsset = null;
        assetLoading = true;
        try {
            selectedAsset = getAssetByGuid(guid);
        } catch (e) {
            console.error("[catalog] getAssetByGuid error:", e);
        } finally {
            assetLoading = false;
        }
    }

    // React to catalogTarget store (GUID navigation from tree)
    $: if ($catalogTarget !== null) {
        console.log("[catalog] catalogTarget fired:", $catalogTarget);
        openDetail($catalogTarget);
        catalogTarget.set(null);
    }

    async function doSearch(q: string, type: string) {
        if (!catalogReady) return;
        if (!q.trim() && !type) {
            results = [];
            return;
        }
        searching = true;
        error = "";
        try {
            results = searchCatalog(q.trim(), 50, type);
        } catch (e) {
            error = String(e);
        } finally {
            searching = false;
        }
    }

    function onInput() {
        clearTimeout(debounceTimer);
        debounceTimer = setTimeout(() => doSearch(query, selectedType), 300);
    }

    function onTypeChange() {
        clearTimeout(debounceTimer);
        doSearch(query, selectedType);
    }

    function isObject(v: unknown): v is Record<string, unknown> {
        return typeof v === "object" && v !== null && !Array.isArray(v);
    }
</script>

<div class="catalog" class:has-detail={selectedAsset !== null || assetLoading}>
    <div class="main-pane">
        <div class="search-bar">
            <input
                class="search-input"
                type="text"
                bind:value={query}
                on:input={onInput}
                placeholder="Search by name or display name…"
                disabled={!catalogReady && !catalogLoading}
            />
            <select
                class="type-select"
                bind:value={selectedType}
                on:change={onTypeChange}
                disabled={!catalogReady || scriptTypes.length === 0}
            >
                <option value="">All types</option>
                {#each scriptTypes as t}
                    <option value={t}>{t}</option>
                {/each}
            </select>
            {#if searching}
                <span class="spin">⟳</span>
            {/if}
        </div>

        {#if catalogLoading}
            <div class="status">Loading catalog.db (~23 MB)…</div>
        {:else if error}
            <div class="error">{error}</div>
        {:else if !catalogReady}
            <div class="status">Catalog not available.</div>
        {:else if (query || selectedType) && results.length === 0 && !searching}
            <div class="status">No results.</div>
        {:else if results.length > 0}
            <table class="results">
                <thead>
                    <tr>
                        <th class="icon-th"></th>
                        <th>GUID</th>
                        <th>Name</th>
                        <th>Display Name</th>
                        <th>Type</th>
                    </tr>
                </thead>
                <tbody>
                    {#each results as row (row.guid)}
                        <tr
                            class:selected={selectedAsset?.guid === row.guid}
                            on:click={() => openDetail(row.guid)}
                        >
                            <td class="icon-td">
                                {#if row.icon}
                                    <img
                                        class="item-icon"
                                        src={row.icon}
                                        alt={row.displayName}
                                    />
                                {/if}
                            </td>
                            <td class="mono">{row.guid}</td>
                            <td class="mono name-col">{row.name}</td>
                            <td>{row.displayName}</td>
                            <td class="type-col">{row.scriptType ?? ""}</td>
                        </tr>
                    {/each}
                </tbody>
            </table>
        {:else if catalogReady && !query && !selectedType}
            <div class="status hint">
                Type to search, or pick a type to browse.
            </div>
        {/if}
    </div>

    {#if assetLoading}
        <div class="detail-pane">
            <div class="detail-loading">Loading…</div>
        </div>
    {:else if selectedAsset}
        <div class="detail-pane">
            <div class="detail-header">
                {#if selectedAsset.icon}
                    <img
                        class="detail-icon"
                        src={selectedAsset.icon}
                        alt={selectedAsset.displayName}
                    />
                {/if}
                <div class="detail-title">
                    <h2 class="detail-name">
                        {selectedAsset.displayName || selectedAsset.name}
                    </h2>
                    {#if selectedAsset.scriptType}
                        <span class="detail-type-badge"
                            >{selectedAsset.scriptType}</span
                        >
                    {/if}
                </div>
                <button
                    class="close-btn"
                    on:click={() => (selectedAsset = null)}
                    title="Close">×</button
                >
            </div>

            <div class="detail-fields">
                <table class="field-table">
                    <tbody>
                        <tr
                            ><td class="field-key">GUID</td><td
                                class="field-val mono">{selectedAsset.guid}</td
                            ></tr
                        >
                        <tr
                            ><td class="field-key">Name</td><td
                                class="field-val mono">{selectedAsset.name}</td
                            ></tr
                        >
                        <tr
                            ><td class="field-key">Bundle</td><td
                                class="field-val mono"
                                >{selectedAsset.bundle ?? "—"}</td
                            ></tr
                        >
                        {#if selectedAsset.scriptType}
                            <tr
                                ><td class="field-key">Script Type</td><td
                                    class="field-val"
                                    >{selectedAsset.scriptType}</td
                                ></tr
                            >
                        {/if}
                    </tbody>
                </table>

                {#if selectedAsset.data && Object.keys(selectedAsset.data).length > 0}
                    <div class="extra-header">Extra fields</div>
                    <table class="field-table">
                        <tbody>
                            {#each Object.entries(selectedAsset.data) as [k, v]}
                                <tr>
                                    <td class="field-key">{k}</td>
                                    <td class="field-val">
                                        {#if isObject(v) || Array.isArray(v)}
                                            <pre
                                                class="field-pre">{JSON.stringify(
                                                    v,
                                                    null,
                                                    2,
                                                )}</pre>
                                        {:else}
                                            <span class="mono">{v}</span>
                                        {/if}
                                    </td>
                                </tr>
                            {/each}
                        </tbody>
                    </table>
                {/if}
            </div>
        </div>
    {/if}
</div>

<style>
    .catalog {
        padding: 16px;
        height: 100%;
        overflow: hidden;
        display: flex;
        flex-direction: row;
        gap: 0;
    }
    .main-pane {
        flex: 1 1 55%;
        min-width: 0;
        overflow: auto;
        display: flex;
        flex-direction: column;
        gap: 12px;
        padding-right: 12px;
    }
    .catalog:not(.has-detail) .main-pane {
        padding-right: 0;
    }
    .detail-pane {
        flex: 0 0 45%;
        min-width: 280px;
        max-width: 520px;
        border-left: 1px solid #2e2e2e;
        padding-left: 16px;
        overflow: auto;
        display: flex;
        flex-direction: column;
        gap: 12px;
    }
    .detail-loading {
        color: #555;
        font-style: italic;
        padding: 16px 0;
    }
    .detail-header {
        display: flex;
        align-items: flex-start;
        gap: 12px;
        padding-bottom: 12px;
        border-bottom: 1px solid #2e2e2e;
    }
    .detail-icon {
        width: 48px;
        height: 48px;
        object-fit: contain;
        flex-shrink: 0;
    }
    .detail-title {
        flex: 1;
        min-width: 0;
    }
    .detail-name {
        font-size: 1em;
        font-weight: 600;
        margin: 0 0 4px;
        color: #ddd;
        word-break: break-word;
    }
    .detail-type-badge {
        font-size: 0.75em;
        color: #569cd6;
        background: rgba(86, 156, 214, 0.12);
        padding: 2px 6px;
        border-radius: 4px;
    }
    .close-btn {
        flex-shrink: 0;
        background: none;
        border: none;
        color: #666;
        font-size: 1.4em;
        line-height: 1;
        cursor: pointer;
        padding: 0 2px;
    }
    .close-btn:hover {
        color: #ccc;
    }
    .detail-fields {
        display: flex;
        flex-direction: column;
        gap: 8px;
    }
    .extra-header {
        font-size: 0.75em;
        color: #666;
        text-transform: uppercase;
        letter-spacing: 0.06em;
        margin-top: 4px;
    }
    .field-table {
        width: 100%;
        border-collapse: collapse;
        font-size: 0.82em;
    }
    .field-table td {
        padding: 3px 6px;
        vertical-align: top;
        border-bottom: 1px solid #1e1e1e;
    }
    .field-key {
        color: #777;
        white-space: nowrap;
        width: 30%;
        font-size: 0.9em;
    }
    .field-val {
        color: #ccc;
        word-break: break-all;
    }
    .field-pre {
        margin: 0;
        white-space: pre-wrap;
        word-break: break-word;
        font-family: "Cascadia Code", "Fira Mono", monospace;
        font-size: 0.9em;
        color: #9cdcfe;
        background: #1a1a1a;
        padding: 4px 6px;
        border-radius: 4px;
    }
    .search-bar {
        display: flex;
        align-items: center;
        gap: 8px;
        flex-wrap: wrap;
    }
    .search-input {
        flex: 1;
        min-width: 200px;
        max-width: 380px;
        background: #1a1a1a;
        border: 1px solid #444;
        border-radius: 6px;
        padding: 8px 12px;
        color: #fff;
        font-size: 1em;
    }
    .search-input:focus {
        outline: none;
        border-color: #646cff;
    }
    .search-input:disabled {
        opacity: 0.5;
    }
    .type-select {
        background: #1a1a1a;
        border: 1px solid #444;
        border-radius: 6px;
        padding: 8px 10px;
        color: #fff;
        font-size: 0.9em;
        max-width: 260px;
    }
    .type-select:focus {
        outline: none;
        border-color: #646cff;
    }
    .type-select:disabled {
        opacity: 0.5;
    }
    .spin {
        color: #646cff;
        font-size: 1.2em;
        animation: spin 1s linear infinite;
    }
    @keyframes spin {
        to {
            transform: rotate(360deg);
        }
    }
    .status {
        color: #666;
        font-style: italic;
        font-size: 0.9em;
    }
    .hint {
        color: #555;
    }
    .error {
        padding: 8px 12px;
        background: rgba(255, 80, 80, 0.1);
        border: 1px solid rgba(255, 80, 80, 0.3);
        border-radius: 6px;
        color: #f88;
        font-size: 0.85em;
    }
    .results {
        width: 100%;
        border-collapse: collapse;
        font-size: 0.85em;
    }
    .results th {
        text-align: left;
        padding: 6px 10px;
        border-bottom: 1px solid #333;
        color: #777;
        font-weight: 500;
        position: sticky;
        top: 0;
        background: #242424;
    }
    .results td {
        padding: 5px 10px;
        border-bottom: 1px solid #1e1e1e;
    }
    .results tr {
        cursor: pointer;
    }
    .results tr:hover td {
        background: rgba(255, 255, 255, 0.03);
    }
    .results tr.selected td {
        background: rgba(100, 108, 255, 0.08);
    }
    .icon-th {
        width: 40px;
    }
    .icon-td {
        width: 40px;
        padding: 3px 6px;
        text-align: center;
    }
    .item-icon {
        width: 32px;
        height: 32px;
        object-fit: contain;
        display: block;
        margin: 0 auto;
    }
    .mono {
        font-family: monospace;
    }
    .name-col {
        color: #9cdcfe;
        max-width: 220px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .type-col {
        color: #888;
        font-size: 0.82em;
        max-width: 200px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
</style>

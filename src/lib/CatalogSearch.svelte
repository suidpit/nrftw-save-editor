<script lang="ts">
    import { catalogTarget } from "./bridge";
    import {
        loadCatalog,
        searchCatalog,
        getScriptTypes,
        getAssetByGuid,
        getEquipmentTargetByGuid,
        getModifierDetailByGuid,
        getPreexistingModifiersForAsset,
        getSiteUrl,
        type CatalogRow,
        type AssetDetail,
    } from "./catalog";

    let query = "";
    let selectedType = "";
    let scriptTypes: string[] = [];
    let results: CatalogRow[] = [];
    let currentPage = 0;
    const pageSize = 25;
    let catalogLoading = false;
    let catalogReady = false;
    let searching = false;
    let error = "";
    let debounceTimer: ReturnType<typeof setTimeout>;

    $: pagedResults = results.slice(currentPage * pageSize, (currentPage + 1) * pageSize);
    $: totalPages = Math.ceil(results.length / pageSize);

    let selectedAsset: AssetDetail | null = null;
    let assetLoading = false;
    let selectedEquipment: ReturnType<typeof getEquipmentTargetByGuid> = null;
    let selectedModifier: ReturnType<typeof getModifierDetailByGuid> = null;
    let selectedLoadout: ReturnType<typeof getPreexistingModifiersForAsset> = [];

    let _catalogPromise: Promise<void> | null = null;

    function ensureCatalog(): Promise<void> {
        if (!_catalogPromise) {
            _catalogPromise = (async () => {
                if (catalogReady) return;
                catalogLoading = true;
                error = "";
                try {
                    await loadCatalog();
                    catalogReady = true;
                    scriptTypes = getScriptTypes();
                } catch (e) {
                    error = `Failed to load catalog: ${e}`;
                } finally {
                    catalogLoading = false;
                }
            })();
        }
        return _catalogPromise;
    }

    // Lazy load catalog on component mount
    void ensureCatalog();

    async function openDetail(guid: string) {
        if (!catalogReady) {
            await ensureCatalog();
        }
        selectedAsset = null;
        assetLoading = true;
        try {
            selectedAsset = getAssetByGuid(guid);
            selectedEquipment = getEquipmentTargetByGuid(guid);
            selectedModifier = getModifierDetailByGuid(guid);
            selectedLoadout = getPreexistingModifiersForAsset(guid, "", 20);
        } catch {
            selectedAsset = null;
            selectedEquipment = null;
            selectedModifier = null;
            selectedLoadout = [];
        } finally {
            assetLoading = false;
        }
    }

    // React to catalogTarget store (GUID navigation from tree)
    $: if ($catalogTarget !== null) {
        openDetail($catalogTarget);
        catalogTarget.set(null);
    }

    function doSearch(q: string, type: string) {
        if (!catalogReady) return;
        if (!q.trim() && !type) {
            results = [];
            currentPage = 0;
            return;
        }
        searching = true;
        error = "";
        try {
            results = searchCatalog(q.trim(), 500, type);
            currentPage = 0;
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
                    {#each pagedResults as row (row.guid)}
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
            {#if totalPages > 1}
                <div class="pagination">
                    <button
                        class="page-btn"
                        disabled={currentPage === 0}
                        on:click={() => (currentPage -= 1)}>‹ Prev</button
                    >
                    <span class="page-info"
                        >{currentPage + 1} / {totalPages} ({results.length} results)</span
                    >
                    <button
                        class="page-btn"
                        disabled={currentPage >= totalPages - 1}
                        on:click={() => (currentPage += 1)}>Next ›</button
                    >
                </div>
            {/if}
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
                        {#if selectedAsset.entryVersion}
                            <tr
                                ><td class="field-key">Catalog Version</td><td
                                    class="field-val"
                                    >{selectedAsset.entryVersion}</td
                                ></tr
                            >
                        {/if}
                        {#if selectedAsset.sitePath}
                            <tr
                                ><td class="field-key">DB Link</td><td class="field-val"
                                    ><a class="detail-link" href={getSiteUrl(selectedAsset.sitePath)} target="_blank" rel="noreferrer">Open site entry</a></td
                                ></tr
                            >
                        {:else if selectedAsset.entryVersion === "v1"}
                            <tr
                                ><td class="field-key">DB Link</td><td class="field-val"
                                    >No site details</td
                                ></tr
                            >
                        {/if}
                    </tbody>
                </table>

                {#if selectedEquipment}
                    <div class="extra-header">Equipment Details</div>
                    <table class="field-table">
                        <tbody>
                            <tr><td class="field-key">Target</td><td class="field-val">{selectedEquipment.targetKind}{selectedEquipment.targetSubkind ? ` / ${selectedEquipment.targetSubkind}` : ""}</td></tr>
                            <tr><td class="field-key">Handling</td><td class="field-val">{selectedEquipment.handling ?? "—"}</td></tr>
                            <tr><td class="field-key">Rune Slots</td><td class="field-val">{selectedEquipment.runeSlots ?? "—"}</td></tr>
                            <tr><td class="field-key">Gem Slots</td><td class="field-val">{selectedEquipment.gemSlots ?? "—"}</td></tr>
                        </tbody>
                    </table>
                {/if}

                {#if selectedModifier}
                    <div class="extra-header">Modifier Details</div>
                    <table class="field-table">
                        <tbody>
                            <tr><td class="field-key">Kind</td><td class="field-val">{selectedModifier.modifierKind ?? "—"}</td></tr>
                            <tr><td class="field-key">Title</td><td class="field-val">{selectedModifier.title}</td></tr>
                            <tr><td class="field-key">Facet</td><td class="field-val">{selectedModifier.isFacet ? "Yes" : "No"}</td></tr>
                            <tr><td class="field-key">Effect</td><td class="field-val">{selectedModifier.effectText ?? "—"}</td></tr>
                            <tr><td class="field-key">Description</td><td class="field-val">{selectedModifier.descriptionText ?? "—"}</td></tr>
                            <tr><td class="field-key">Only on</td><td class="field-val">{selectedModifier.onlyOnItems ?? "—"}</td></tr>
                        </tbody>
                    </table>
                {/if}

                {#if selectedLoadout.length > 0}
                    <div class="extra-header">Pre-existing Modifiers</div>
                    <table class="field-table">
                        <tbody>
                            {#each selectedLoadout as modifier}
                                <tr>
                                    <td class="field-key">{modifier.modifierKind}</td>
                                    <td class="field-val">{modifier.displayName || modifier.name}</td>
                                </tr>
                            {/each}
                        </tbody>
                    </table>
                {/if}

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
    .detail-link {
        color: #c8a050;
        text-decoration: none;
    }
    .detail-link:hover {
        text-decoration: underline;
    }
    .field-pre {
        margin: 0;
        white-space: pre-wrap;
        word-break: break-word;
        font-family: var(--font-mono);
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
    .pagination {
        display: flex;
        align-items: center;
        gap: 10px;
        padding: 8px 4px;
        border-top: 1px solid #2e2e2e;
    }
    .page-btn {
        background: #1a1a1a;
        border: 1px solid #444;
        border-radius: 4px;
        color: #ccc;
        padding: 4px 10px;
        font-size: 0.85em;
        cursor: pointer;
    }
    .page-btn:hover:not(:disabled) {
        border-color: #646cff;
        color: #fff;
    }
    .page-btn:disabled {
        opacity: 0.35;
        cursor: default;
    }
    .page-info {
        flex: 1;
        text-align: center;
        font-size: 0.82em;
        color: #666;
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

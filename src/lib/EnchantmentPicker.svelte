<script lang="ts">
    import { createEventDispatcher } from "svelte";
    import {
        searchCompatibleEnchantmentsForAsset,
        type CompatibleEnchantmentRow,
    } from "./catalog";

    export let assetGuid: string;
    export let excludeGuids: string[] = [];

    const dispatch = createEventDispatcher<{
        close: void;
        select: CompatibleEnchantmentRow;
    }>();

    let query = "";
    let results: CompatibleEnchantmentRow[] = [];

    $: if (assetGuid) {
        search();
    }

    function search() {
        results = searchCompatibleEnchantmentsForAsset(assetGuid, query, 30).filter(
            (row) => !excludeGuids.includes(row.guid),
        );
    }

    function select(row: CompatibleEnchantmentRow) {
        dispatch("select", row);
    }

    function close() {
        dispatch("close");
    }
</script>

<div class="modal-backdrop" on:click={close} role="presentation">
    <div class="modal" on:click|stopPropagation role="dialog" aria-modal="true" tabindex="-1" on:keydown={e => e.key === "Escape" && close()}>
        <div class="modal-header">
            <h2 class="modal-title">Add Enchantment</h2>
            <button class="close-btn" on:click={close} aria-label="Close">✕</button>
        </div>
        <div class="modal-body">
            <input
                class="picker-input"
                type="text"
                placeholder="Search enchantments…"
                bind:value={query}
                on:input={search}
            />
            <div class="picker-list">
                {#each results as row (row.guid)}
                    <button class="picker-row" on:click={() => select(row)}>
                        <span class="picker-primary">
                            {#if row.icon}
                                <img src={row.icon} alt="" class="picker-icon" />
                            {/if}
                            <span>{row.displayName || row.title || row.name}</span>
                        </span>
                        {#if row.effectText}
                            <span class="picker-secondary">{row.effectText}</span>
                        {/if}
                        {#if row.onlyOnItems && row.onlyOnItems !== "Regular Item"}
                            <span class="picker-secondary">Only on: {row.onlyOnItems}</span>
                        {/if}
                    </button>
                {:else}
                    <div class="picker-empty">
                        {query ? "No results." : "Type to search enchantments."}
                    </div>
                {/each}
            </div>
        </div>
    </div>
</div>

<style>
    .modal-backdrop {
        position: fixed;
        inset: 0;
        background: rgba(0, 0, 0, 0.7);
        display: flex;
        align-items: center;
        justify-content: center;
        z-index: 600;
    }

    .modal {
        background: var(--bg-panel, #231a0e);
        border: 1px solid var(--border-color, #4a3520);
        border-radius: 10px;
        width: 360px;
        max-width: 95vw;
        max-height: 85vh;
        display: flex;
        flex-direction: column;
        box-shadow: 0 8px 40px rgba(0, 0, 0, 0.8);
    }

    .modal-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 14px 16px 12px;
        border-bottom: 1px solid var(--border-color, #4a3520);
        flex-shrink: 0;
    }

    .modal-title {
        font-family: Georgia, serif;
        font-size: 0.95em;
        color: var(--accent-gold, #c8a050);
        margin: 0;
        font-weight: 600;
    }

    .close-btn {
        background: none;
        border: none;
        color: var(--text-secondary, #a89070);
        font-size: 0.9em;
        padding: 2px 6px;
        border-radius: 4px;
    }

    .close-btn:hover {
        color: var(--text-primary, #e8d5a3);
    }

    .modal-body {
        flex: 1;
        overflow-y: auto;
        padding: 16px;
        display: flex;
        flex-direction: column;
        gap: 14px;
    }

    .picker-input {
        width: 100%;
        padding: 6px 8px;
        font-size: 0.85em;
        background: var(--bg-card, #2d2010);
        border: 1px solid var(--border-color, #4a3520);
        border-radius: 4px;
        color: var(--text-primary, #e8d5a3);
        font-family: inherit;
    }

    .picker-input:focus {
        border-color: var(--accent-gold, #c8a050);
        outline: none;
    }

    .picker-list {
        display: flex;
        flex-direction: column;
        gap: 4px;
        max-height: 320px;
        overflow-y: auto;
    }

    .picker-row {
        display: flex;
        align-items: center;
        gap: 8px;
        justify-content: space-between;
        text-align: left;
        border: 1px solid var(--border-color, #4a3520);
        background: var(--bg-card, #2d2010);
        color: var(--text-primary, #e8d5a3);
        border-radius: 4px;
        padding: 6px 8px;
        font-family: inherit;
    }

    .picker-primary {
        display: flex;
        align-items: center;
        gap: 8px;
        min-width: 0;
    }

    .picker-icon {
        width: 28px;
        height: 28px;
        object-fit: contain;
        flex-shrink: 0;
    }

    .picker-secondary {
        font-size: 0.76em;
        color: var(--text-secondary, #a89070);
    }

    .picker-empty {
        font-size: 0.78em;
        color: var(--text-secondary, #a89070);
        padding: 6px 2px;
    }
</style>

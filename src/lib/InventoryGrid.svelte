<script lang="ts">
    import { tick } from "svelte";
    import { catalogLoaded, getAssetByGuid, getModifierDetailByGuid } from "./catalog";
    import { isEquipmentAssetType, isStackableAssetType, RARITY_NAMES, RARITY_LABELS, RARITY_COLORS } from "./inventory-assets";
    import {
        deleteItemChange,
        getInventorySnapshot,
        parseInventoryIndex,
        pendingItems,
        stageItemChange,
        type DocMeta,
        type InventorySnapshotItem,
    } from "./bridge";
    import ItemEditor from "./ItemEditor.svelte";
    import type {
        InventoryViewItem,
        ItemEditorDraft,
        PendingItemChanges,
    } from "./types";

    export let docs: DocMeta[];

    const SLOT_NAMES: Record<number, string> = {
        0: "Helmet",
        1: "Body",
        2: "Pants",
        3: "Gloves",
        4: "L.Hand (unarmed)",
        5: "L.Hand 1",
        6: "L.Hand 2",
        7: "L.Hand 3",
        8: "R.Hand (unarmed)",
        9: "R.Hand 1",
        10: "R.Hand 2",
        11: "R.Hand 3",
        12: "Ring 1",
        13: "Ring 2",
        14: "Ring 3",
        19: "Quick Item",
        24: "Arrows",
        25: "Bolts",
        26: "Food",
        27: "Seed Bag",
        28: "Tool 1",
        29: "Tool 2",
        30: "Tool 3",
        31: "Tool 4",
        32: "Tool 5",
        33: "Tool 6",
        [-1]: "None",
    };

    let baseSnapshotItems: InventorySnapshotItem[] = [];
    let baseItems: InventoryViewItem[] = [];
    let displayedItems: InventoryViewItem[] = [];
    let loading = true;
    let lastLoadedDocIndex: number | undefined;
    const TOOLTIP_GAP = 14;
    const VIEWPORT_MARGIN = 8;

    let tooltip: { item: InventoryViewItem; x: number; y: number } | null = null;
    let tooltipElement: HTMLDivElement | null = null;
    let editingItem: ItemEditorDraft | null = null;
    let editorMode: "edit" | "create" = "edit";
    let nextDraftId = 1;

    $: contentDoc = docs.find(
        (d) => d.rootType === "Quantum.CharacterContents",
    );

    $: if (
        contentDoc !== undefined &&
        contentDoc.index !== lastLoadedDocIndex
    ) {
        lastLoadedDocIndex = contentDoc.index;
        loadInventory(contentDoc);
    }

    $: {
        $catalogLoaded;
        baseItems = contentDoc
            ? baseSnapshotItems.map((item) => toInventoryViewItem(contentDoc.index, item))
            : [];
    }
    $: displayedItems = mergeItems(baseItems, $pendingItems);

    async function loadInventory(doc: DocMeta) {
        loading = true;
        baseSnapshotItems = await getInventorySnapshot(doc.index);
        loading = false;
    }

    function toInventoryViewItem(
        docIdx: number,
        snapshot: InventorySnapshotItem,
    ): InventoryViewItem {
        const asset = snapshot.assetGuid
            ? getAssetByGuid(snapshot.assetGuid)
            : null;
        const displayName =
            asset?.displayName ??
            asset?.name ??
            (snapshot.assetGuid
                ? snapshot.assetGuid.slice(0, 8) + "…"
                : "Unknown");

        return {
            docIdx,
            itemPath: snapshot.itemPath,
            draftId: snapshot.itemPath,
            assetGuid: snapshot.assetGuid,
            assetType: asset?.scriptType ?? "",
            displayName,
            icon: asset?.icon ?? null,
            level: snapshot.level,
            rarityNum: snapshot.rarityNum,
            durability: snapshot.durability,
            stackCount: snapshot.stackCount,
            runeGuids: [...snapshot.runeGuids],
            runeNames: snapshot.runeGuids.map(runeName),
            enchantmentGuids: [...snapshot.enchantmentGuids],
            enchantmentNames: snapshot.enchantmentGuids.map(enchantmentName),
            traitGuid: snapshot.traitGuid,
            traitName: traitName(snapshot.traitGuid),
            isNew: false,
            index: snapshot.index,
            slotNum: snapshot.slotNum,
            rarityClass: RARITY_NAMES[snapshot.rarityNum] ?? "white",
        };
    }

    function mergeItems(
        items: InventoryViewItem[],
        pending: PendingItemChanges,
    ): InventoryViewItem[] {
        const merged = items.map((item) => ({ ...item }));

        for (const deletion of pending.deletes) {
            const matchIndex = parseInventoryIndex(deletion.itemPath);
            if (matchIndex !== null && matchIndex >= 0 && matchIndex < merged.length) {
                merged.splice(matchIndex, 1);
            }
        }

        const rebased: InventoryViewItem[] = merged.map((item, index) => {
            const itemPath = `Inventory[${index}]`;
            const pendingEdit = pending.edits[itemPath];
            if (!pendingEdit) {
                return {
                    ...item,
                    itemPath,
                    index,
                };
            }

            const rarityClass = RARITY_NAMES[pendingEdit.rarityNum] ?? "white";
            return {
                ...item,
                ...pendingEdit,
                itemPath,
                draftId: pendingEdit.draftId ?? item.draftId,
                index,
                rarityClass,
                isNew: false,
            };
        });

        const created: InventoryViewItem[] = pending.creates.map((draft, offset) => ({
            ...draft,
            index: rebased.length + offset,
            draftId: draft.draftId ?? `new-${offset}`,
            slotNum: -1,
            rarityClass: RARITY_NAMES[draft.rarityNum] ?? "white",
        }));

        return [...rebased, ...created];
    }

    function getTooltipPosition(event: MouseEvent): { x: number; y: number } {
        const tooltipRect = tooltipElement?.getBoundingClientRect();
        const tooltipWidth = tooltipRect?.width ?? 280;
        const tooltipHeight = tooltipRect?.height ?? 0;
        const preferredX = event.clientX + TOOLTIP_GAP;
        const fallbackX = event.clientX - tooltipWidth - TOOLTIP_GAP;
        const preferredY = event.clientY + TOOLTIP_GAP;
        const maxX = window.innerWidth - tooltipWidth - VIEWPORT_MARGIN;
        const maxY = window.innerHeight - tooltipHeight - VIEWPORT_MARGIN;
        const x =
            preferredX + tooltipWidth <= window.innerWidth - VIEWPORT_MARGIN
                ? preferredX
                : Math.max(VIEWPORT_MARGIN, Math.min(fallbackX, maxX));
        const y = Math.max(VIEWPORT_MARGIN, Math.min(preferredY, maxY));

        return { x, y };
    }

    async function showTooltip(event: MouseEvent, item: InventoryViewItem) {
        tooltip = { item, ...getTooltipPosition(event) };
        await tick();

        if (tooltip?.item === item) {
            tooltip = { item, ...getTooltipPosition(event) };
        }
    }

    function moveTooltip(event: MouseEvent) {
        if (tooltip)
            tooltip = {
                ...tooltip,
                ...getTooltipPosition(event),
            };
    }

    function hideTooltip() {
        tooltip = null;
    }

    function slotLabel(slotNum: number): string {
        return SLOT_NAMES[slotNum] ?? `Slot ${slotNum}`;
    }

    function isEquipmentItem(item: Pick<InventoryViewItem, "assetType">): boolean {
        return isEquipmentAssetType(item.assetType);
    }

    function isStackableItem(item: Pick<InventoryViewItem, "assetType">): boolean {
        return isStackableAssetType(item.assetType);
    }

    function rarityLabel(rarityNum: number): string {
        return RARITY_LABELS[rarityNum] ?? "Normal";
    }

    function openEditItem(item: InventoryViewItem) {
        tooltip = null;
        editingItem = {
            docIdx: item.docIdx,
            itemPath: item.itemPath,
            assetGuid: item.assetGuid,
            assetType: item.assetType,
            displayName: item.displayName,
            icon: item.icon,
            level: item.level,
            rarityNum: item.rarityNum,
            durability: item.durability,
            stackCount: item.stackCount,
            runeGuids: [...item.runeGuids],
            runeNames: [...item.runeNames],
            enchantmentGuids: [...item.enchantmentGuids],
            enchantmentNames: [...item.enchantmentNames],
            traitGuid: item.traitGuid,
            traitName: item.traitName,
            isNew: item.isNew,
            draftId: item.draftId,
        };
        editorMode = "edit";
    }

    function openCreateItem() {
        if (!contentDoc) return;
        tooltip = null;
        editingItem = {
            docIdx: contentDoc.index,
            itemPath: null,
            assetGuid: "",
            assetType: "",
            displayName: "",
            icon: null,
            level: 1,
            rarityNum: 0,
            durability: 100,
            stackCount: 1,
            runeGuids: [],
            runeNames: [],
            enchantmentGuids: [],
            enchantmentNames: [],
            traitGuid: "",
            traitName: "",
            isNew: true,
            draftId: `draft-${nextDraftId++}`,
        };
        editorMode = "create";
    }

    function enchantmentName(guid: string): string {
        const enchantment = getModifierDetailByGuid(guid) ?? getAssetByGuid(guid);
        return (
            enchantment?.displayName ||
            enchantment?.name ||
            (guid ? guid.slice(0, 6) + "…" : "Unknown")
        );
    }

    function runeName(guid: string): string {
        const rune = getAssetByGuid(guid);
        return rune?.displayName || rune?.name || (guid ? guid.slice(0, 6) + "…" : "Unknown");
    }

    function traitName(guid: string): string {
        if (!guid) return "";
        const trait = getModifierDetailByGuid(guid) ?? getAssetByGuid(guid);
        return trait?.displayName || trait?.name || (guid ? guid.slice(0, 6) + "…" : "Unknown");
    }

    function closeEditor() {
        editingItem = null;
    }

    function handleSave(event: CustomEvent<ItemEditorDraft>) {
        stageItemChange(event.detail);
        editingItem = null;
    }

    function handleDelete(event: CustomEvent<ItemEditorDraft>) {
        deleteItemChange(event.detail);
        editingItem = null;
    }
</script>

<div class="inventory-grid-wrapper">
    <div class="panel-header">
        <h3 class="panel-title">Inventory ({displayedItems.length})</h3>
    </div>
    {#if loading}
        <div class="loading">Loading inventory…</div>
    {:else if displayedItems.length === 0}
        <div class="empty">No items found.</div>
    {:else}
        <div class="inventory-grid">
            {#each displayedItems as item (item.itemPath ?? item.draftId)}
                {@const rc = RARITY_COLORS[item.rarityClass]}
                <div class="item-wrap">
                    <div
                        class="inventory-item rarity-{item.rarityClass}"
                        class:pending-item={item.isNew ||
                            !!$pendingItems.edits[item.itemPath ?? ""]}
                        style="border-color: {rc}"
                        on:mouseenter={(e) => showTooltip(e, item)}
                        on:mousemove={moveTooltip}
                        on:mouseleave={hideTooltip}
                        on:click={() => openEditItem(item)}
                        on:keydown={(e) =>
                            (e.key === "Enter" || e.key === " ") &&
                            openEditItem(item)}
                        role="button"
                        tabindex="0"
                        aria-label={item.displayName}
                    >
                        {#if item.icon}
                            <img
                                class="item-icon"
                                src={item.icon}
                                alt={item.displayName}
                            />
                        {:else}
                            <span class="item-placeholder">?</span>
                        {/if}
                        {#if item.stackCount > 1}
                            <span class="item-stack">{item.stackCount}</span>
                        {/if}
                    </div>
                </div>
            {/each}

            <div
                class="inventory-item add-item"
                role="button"
                tabindex="0"
                aria-label="Add item"
                on:click={openCreateItem}
                on:keydown={(e) => e.key === "Enter" && openCreateItem()}
            >
                <span class="add-icon">+</span>
            </div>
        </div>
    {/if}
</div>

{#if tooltip}
    {@const item = tooltip.item}
    {@const rc = RARITY_COLORS[item.rarityClass]}
    {@const slot = slotLabel(item.slotNum)}
    {@const equipmentItem = isEquipmentItem(item)}
    {@const stackableItem = isStackableItem(item)}
    <div
        bind:this={tooltipElement}
        class="item-tooltip"
        style="left: {tooltip.x}px; top: {tooltip.y}px;"
    >
        <div class="tooltip-header">
            <strong class="tooltip-name" style="color: {rc}"
                >{item.displayName}</strong
            >
            <span
                class="tooltip-rarity"
                style="color: {rc}; border-color: {rc}44"
            >
                {rarityLabel(item.rarityNum)}
            </span>
        </div>
        <div class="tooltip-meta">
            {#if slot !== "None"}
                <span>{slot}</span>
            {/if}
            {#if item.level > 0}
                <span>Lv {item.level}</span>
            {/if}
            {#if stackableItem && item.stackCount > 1}
                <span>x{item.stackCount}</span>
            {/if}
            {#if equipmentItem}
                <span>Dur: {item.durability === -1 ? "∞" : item.durability}</span>
            {/if}
        </div>
        {#if equipmentItem && item.traitName}
            <div class="tooltip-enchants">
                <div class="tooltip-enchant tooltip-trait">Facet: {item.traitName}</div>
            </div>
        {/if}
        {#if equipmentItem && item.runeNames.length > 0}
            <div class="tooltip-enchants">
                {#each item.runeNames as name}
                    <div class="tooltip-enchant">Rune: {name}</div>
                {/each}
            </div>
        {/if}
        {#if equipmentItem && item.enchantmentNames.length > 0}
            <div class="tooltip-enchants">
                {#each item.enchantmentNames as name}
                    <div class="tooltip-enchant">{name}</div>
                {/each}
            </div>
        {/if}
        <code class="tooltip-guid">{item.assetGuid}</code>
    </div>
{/if}

{#if editingItem}
    <ItemEditor
        draft={editingItem}
        mode={editorMode}
        on:close={closeEditor}
        on:delete={handleDelete}
        on:save={handleSave}
    />
{/if}

<style>
    .inventory-grid-wrapper {
        padding: 16px;
        overflow-y: auto;
        height: 100%;
    }

    .panel-header {
        display: flex;
        align-items: baseline;
        justify-content: space-between;
        gap: 12px;
        margin-bottom: 12px;
    }

    .panel-title {
        font-family: Georgia, serif;
        font-size: 0.72em;
        font-weight: 600;
        color: var(--accent-gold, #c8a050);
        text-transform: uppercase;
        letter-spacing: 0.08em;
        margin: 0;
        padding-bottom: 4px;
        border-bottom: 1px solid var(--border-color, #4a3520);
    }

    .panel-note,
    .loading,
    .empty {
        color: var(--text-secondary, #a89070);
        font-size: 0.82em;
    }

    .inventory-grid {
        display: grid;
        grid-template-columns: repeat(auto-fill, minmax(78px, 1fr));
        gap: 10px;
    }

    .item-wrap {
        position: relative;
    }

    .inventory-item {
        aspect-ratio: 1;
        border: 1px solid var(--border-color, #4a3520);
        border-radius: 8px;
        background: var(--bg-card, #2d2010);
        display: flex;
        align-items: center;
        justify-content: center;
        position: relative;
        transition:
            transform 0.15s,
            border-color 0.15s;
    }

    .inventory-item:hover {
        transform: translateY(-1px);
    }

    .pending-item {
        box-shadow: 0 0 0 1px rgba(200, 160, 80, 0.4);
    }

    .item-icon {
        width: 56px;
        height: 56px;
        object-fit: contain;
    }

    .item-placeholder,
    .add-icon {
        font-size: 1.4em;
        color: var(--text-secondary, #a89070);
    }

    .item-stack {
        position: absolute;
        right: 6px;
        bottom: 5px;
        min-width: 18px;
        padding: 1px 4px;
        border-radius: 999px;
        background: rgba(0, 0, 0, 0.78);
        color: var(--text-primary, #e8d5a3);
        font-size: 0.72em;
        line-height: 1.2;
        text-align: center;
        box-shadow: 0 0 0 1px rgba(200, 160, 80, 0.2);
    }

    .add-item {
        border-style: dashed;
    }

    .item-tooltip {
        position: fixed;
        z-index: 80;
        min-width: 220px;
        max-width: 280px;
        padding: 10px 12px;
        border-radius: 8px;
        border: 1px solid var(--border-color, #4a3520);
        background: rgba(20, 14, 7, 0.96);
        box-shadow: 0 10px 24px rgba(0, 0, 0, 0.45);
        pointer-events: none;
    }

    .tooltip-header,
    .tooltip-meta {
        display: flex;
        align-items: baseline;
        justify-content: space-between;
        gap: 10px;
    }

    .tooltip-name {
        font-size: 0.9em;
    }

    .tooltip-rarity {
        font-size: 0.72em;
        text-transform: uppercase;
        padding: 1px 6px;
        border-radius: 999px;
        border: 1px solid currentColor;
    }

    .tooltip-meta,
    .tooltip-guid,
    .tooltip-enchant {
        font-size: 0.76em;
        color: var(--text-secondary, #a89070);
    }

    .tooltip-enchants {
        margin-top: 8px;
        display: flex;
        flex-direction: column;
        gap: 2px;
    }

    .tooltip-guid {
        display: block;
        margin-top: 8px;
        white-space: normal;
    }
</style>

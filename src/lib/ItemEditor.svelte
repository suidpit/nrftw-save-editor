<script lang="ts">
    import { createEventDispatcher } from "svelte";
    import {
        catalogLoaded,
        getAssetByGuid,
        getCatalogEntryMeta,
        getEquipmentTargetForEditing,
        getModifierDetailByGuid,
        getPreexistingModifiersForAsset,
        getSiteUrl,
        hasCatalogPreexistingModifiers,
        searchCatalog,
        type CatalogRow,
        type CompatibleEnchantmentRow,
        type ModifierDetail,
    } from "./catalog";
    import { computeRolledValue, formatRollText, qualityFromDesiredValue } from "./quality";
    import EnchantmentPicker from "./EnchantmentPicker.svelte";
    import {
        INVENTORY_ASSET_TYPES,
        RARITY_LABELS,
        isEquipmentAssetType,
        isInventoryAssetType,
        isStackableAssetType,
    } from "./inventory-assets";
    import type { EnchantmentEntry, ItemEditorDraft } from "./types";
    import { isKnownUniqueItem } from "./unique-items";

    export let draft: ItemEditorDraft;
    export let mode: "edit" | "create" = "edit";

    const dispatch = createEventDispatcher<{
        close: void;
        delete: ItemEditorDraft;
        save: ItemEditorDraft;
    }>();

    const GOLD_RARITY = 3;
    const MAX_NON_UNIQUE_RARITY = 2;
    const CREATE_DURABILITY = 100;
    const MAX_EXALT_TOTAL = 4;
    const ENCHANT_CAP_BLUE = 3;   // rarity 1
    const ENCHANT_CAP_PURPLE = 5; // rarity 2

    let assetType = draft.assetType || "";
    let assetGuid = draft.assetGuid;
    let assetName = draft.assetGuid ? draft.displayName : "";
    let assetIcon = draft.assetGuid ? draft.icon : null;
    let level = draft.level;
    let rarity = draft.rarityNum;
    let durability = draft.durability;
    let stackCount = draft.stackCount;
    let runeGuids = [...draft.runeGuids];
    let runeNames = [...draft.runeNames];
    let enchantments: EnchantmentEntry[] = draft.enchantments.map(e => ({ ...e }));
    let traitGuid = draft.traitGuid;
    let traitName = draft.traitName;

    let assetPickerQuery = "";
    let assetPickerResults: CatalogRow[] = [];
    let showAssetPicker = false;

    let showEnchantSearch = false;

    let saveError = "";
    const isCreateMode = mode === "create";
    $: isEquipmentItem = isEquipmentAssetType(assetType);
    $: isWeaponItem = assetType === "WeaponStaticDataAsset";
    $: isRingItem = assetType === "RingsDataAsset";
    $: isStackableItem = isStackableAssetType(assetType);
    $: catalogEntryMeta = assetGuid ? getCatalogEntryMeta(assetGuid) : null;
    $: equipmentTarget = assetGuid ? getEquipmentTargetForEditing(assetGuid, assetType) : null;
    $: siteUrl = getSiteUrl(catalogEntryMeta?.sitePath);
    $: hasCatalogDefaults = assetGuid ? hasCatalogPreexistingModifiers(assetGuid) : false;
    $: isKnownUnique = assetGuid ? isKnownUniqueItem(assetGuid) : false;
    $: totalExaltStacks = enchantments.reduce((sum, e) => sum + e.exaltStacks, 0);
    $: enchantmentCap = isKnownUnique ? enchantments.length
        : rarity >= 2 ? ENCHANT_CAP_PURPLE
        : rarity >= 1 ? ENCHANT_CAP_BLUE
        : 0;
    $: isFullySlotted = isKnownUnique || enchantments.length >= enchantmentCap;
    $: modifiersLocked = isRingItem || isKnownUnique;
    $: goldAllowed = isKnownUnique;
    $: availableRarityIndexes = goldAllowed ? [0, 1, 2, 3] : [0, 1, 2];
    $: assetTypeLabel =
        INVENTORY_ASSET_TYPES.find((opt) => opt.value === assetType)?.label ??
        assetType ??
        "Unknown";
    $: traitDetail = traitGuid ? getModifierDetailByGuid(traitGuid) : null;
    $: currentRuneDetails = runeGuids
        .map((guid) => getAssetByGuid(guid))
        .filter((detail): detail is NonNullable<typeof detail> => detail !== null);
    $: minimumRarityForEnchantments = getMinimumRarityForEnchantments(
        enchantments.length,
    );
    $: rarityWasAdjustedForEnchantments =
        enchantments.length > 0 && rarity === minimumRarityForEnchantments;
    $: if (isCreateMode && durability !== CREATE_DURABILITY) {
        durability = CREATE_DURABILITY;
    }
    $: if (modifiersLocked && showEnchantSearch) {
        showEnchantSearch = false;
    }
    $: if (!goldAllowed && rarity === GOLD_RARITY) {
        rarity = normalizeRarityForEnchantments(
            MAX_NON_UNIQUE_RARITY,
            enchantments.length,
        );
    }
    $: if (!isEquipmentItem && enchantments.length > 0) {
        enchantments = [];
    }
    $: if (!isWeaponItem && runeGuids.length > 0) {
        runeGuids = [];
        runeNames = [];
    }
    $: if (!isStackableItem && stackCount !== 1) {
        stackCount = 1;
    }
    $: if (showAssetPicker && $catalogLoaded) {
        searchAssets();
    }

    function searchAssets() {
        if (!isCreateMode) return;
        assetPickerResults = searchCatalog(assetPickerQuery, 100, assetType).filter(
            isSelectableCreateAsset,
        );
    }

    function isSelectableCreateAsset(row: CatalogRow): boolean {
        if (assetType) {
            return row.scriptType === assetType;
        }
        return isInventoryAssetType(row.scriptType);
    }

    function selectAsset(row: CatalogRow) {
        if (!isCreateMode) return;
        assetGuid = row.guid;
        assetName = row.displayName || row.name;
        assetIcon = row.icon;
        assetType = row.scriptType ?? assetType;
        applyCatalogDefaults(row.guid);
        showAssetPicker = false;
        assetPickerQuery = "";
    }

    function applyCatalogDefaults(nextAssetGuid: string) {
        const defaultRunes = getPreexistingModifiersForAsset(nextAssetGuid, "rune");
        const defaultEnchants = getPreexistingModifiersForAsset(nextAssetGuid, "enchantment");
        runeGuids = defaultRunes.map((modifier) => modifier.guid);
        runeNames = defaultRunes.map((modifier) => modifier.displayName || modifier.name);
        enchantments = defaultEnchants.map((modifier) => ({
            guid: modifier.guid,
            name: modifier.displayName || modifier.name,
            quality: defaultQualityFor(modifier.guid),
            exaltStacks: 0,
        }));
        if (isKnownUniqueItem(nextAssetGuid)) {
            rarity = GOLD_RARITY;
            return;
        }

        rarity = normalizeRarityForEnchantments(0, defaultEnchants.length);
    }

    function defaultQualityFor(guid: string): string {
        const detail = getModifierDetailByGuid(guid);
        if (detail?.rollKind === "range" && detail.rollMin != null && detail.rollMax != null) {
            return qualityFromDesiredValue(detail.rollMax, detail.rollMin, detail.rollMax, detail.rollIsNegative);
        }
        return "0";
    }

    function addEnchantment(row: CompatibleEnchantmentRow) {
        if (modifiersLocked) return;
        if (enchantments.some(e => e.guid === row.guid)) return;
        const name = row.displayName || row.title || row.name;
        enchantments = [...enchantments, {
            guid: row.guid,
            name,
            quality: defaultQualityFor(row.guid),
            exaltStacks: 0,
        }];
        rarity = normalizeRarityForEnchantments(rarity, enchantments.length);
        showEnchantSearch = false;
    }

    function enchantmentDetail(guid: string): ModifierDetail | null {
        return getModifierDetailByGuid(guid);
    }

    function removeEnchantment(index: number) {
        if (modifiersLocked) return;
        enchantments = enchantments.filter((_, i) => i !== index);
        rarity = normalizeRarityForEnchantments(rarity, enchantments.length);
    }

    function moveEnchantment(index: number, direction: -1 | 1) {
        if (modifiersLocked) return;
        const nextIndex = index + direction;
        if (nextIndex < 0 || nextIndex >= enchantments.length) return;
        const next = [...enchantments];
        [next[index], next[nextIndex]] = [next[nextIndex], next[index]];
        enchantments = next;
    }

    function enchantmentRolledText(entry: EnchantmentEntry): string | null {
        const detail = getModifierDetailByGuid(entry.guid);
        if (!detail) return null;
        const result = computeRolledValue(
            entry.quality,
            detail.rollKind,
            detail.rollMin,
            detail.rollMax,
            detail.rollValue,
            detail.rollUnit,
            detail.rollIsNegative,
        );
        return formatRollText(detail.rollText, result);
    }

    function setExaltStacks(index: number, value: number) {
        const clamped = Math.max(0, Math.min(4, value));
        const otherTotal = enchantments.reduce((sum, e, i) => i === index ? sum : sum + e.exaltStacks, 0);
        const maxForThis = MAX_EXALT_TOTAL - otherTotal;
        const next = [...enchantments];
        next[index] = { ...next[index], exaltStacks: Math.min(clamped, maxForThis) };
        enchantments = next;
    }

    function setEnchantmentRoll(index: number, desiredInt: number) {
        const entry = enchantments[index];
        const detail = getModifierDetailByGuid(entry.guid);
        if (!detail || detail.rollKind !== "range" || detail.rollMin == null || detail.rollMax == null) return;
        const newQuality = qualityFromDesiredValue(desiredInt, detail.rollMin, detail.rollMax, detail.rollIsNegative);
        const next = [...enchantments];
        next[index] = { ...entry, quality: newQuality };
        enchantments = next;
    }

    function save() {
        if (!assetGuid) {
            saveError = "Select an asset first.";
            return;
        }

        const asset = getAssetByGuid(assetGuid);
        dispatch("save", {
            ...draft,
            assetGuid: isCreateMode ? assetGuid : draft.assetGuid,
            assetType: isCreateMode ? assetType : draft.assetType,
            displayName: isCreateMode ? assetName || asset?.displayName || asset?.name || assetGuid : draft.displayName,
            icon: isCreateMode ? assetIcon ?? asset?.icon ?? null : draft.icon,
            level,
            rarityNum: rarity,
            durability: isEquipmentItem ? (isCreateMode ? CREATE_DURABILITY : durability) : 0,
            stackCount: isStackableItem ? Math.max(1, Math.floor(stackCount || 1)) : 1,
            runeGuids: isWeaponItem ? runeGuids : [],
            runeNames: isWeaponItem ? runeNames : [],
            enchantments: isEquipmentItem ? enchantments : [],
            traitGuid,
            traitName,
        });
    }

    function close() {
        dispatch("close");
    }

    function removeItem() {
        const confirmed = window.confirm(
            isCreateMode
                ? "Discard this staged item?"
                : "Delete this item from the inventory?",
        );
        if (!confirmed) return;
        dispatch("delete", draft);
    }

    function getMinimumRarityForEnchantments(enchantmentCount: number) {
        if (enchantmentCount === 0) return 0;
        if (enchantmentCount <= 3) return 1;
        return 2;
    }

    function normalizeRarityForEnchantments(currentRarity: number, enchantmentCount: number) {
        if (currentRarity === GOLD_RARITY) return GOLD_RARITY;
        return Math.max(currentRarity, getMinimumRarityForEnchantments(enchantmentCount));
    }
</script>

<div class="modal-backdrop" on:click={close} role="presentation">
    <div class="modal" on:click|stopPropagation role="dialog" aria-modal="true" tabindex="-1" on:keydown={e => e.key === "Escape" && close()}>
        <div class="modal-header">
            <h2 class="modal-title">
                {mode === "create" ? "Create Item" : `Edit: ${draft.displayName}`}
            </h2>
            <button class="close-btn" on:click={close} aria-label="Close">✕</button>
        </div>

        <div class="modal-body">
            <section class="field-section">
                <label class="field-label" for="asset-type">Asset Type</label>
                {#if isCreateMode}
                    <select
                        id="asset-type"
                        class="field-select"
                        bind:value={assetType}
                        on:change={() => {
                            assetGuid = "";
                            assetName = "";
                            assetIcon = null;
                            runeGuids = [];
                            runeNames = [];
                            enchantments = [];
                            traitGuid = "";
                            traitName = "";
                            assetPickerResults = [];
                        }}
                    >
                        <option value="">Any</option>
                        {#each INVENTORY_ASSET_TYPES as opt}
                            <option value={opt.value}>{opt.label}</option>
                        {/each}
                    </select>
                {:else}
                    <div class="selected-asset">
                        <span class="selected-name">{assetTypeLabel}</span>
                    </div>
                {/if}
            </section>

            <section class="field-section">
                <span class="field-label">Asset</span>
                {#if assetName}
                    <div class="selected-asset">
                        <div class="selected-meta">
                            {#if assetIcon}
                                <img src={assetIcon} alt="" class="picker-icon" />
                            {/if}
                            <span class="selected-name">{assetName}</span>
                            {#if siteUrl}
                                (<a class="catalog-link" href={siteUrl} target="_blank" rel="noreferrer">norestforthewicked.gg</a>)
                            {/if}
                        </div>
                    </div>
                {:else}
                    <button class="small-btn primary" on:click={() => { showAssetPicker = true; searchAssets(); }}>
                        Pick Asset…
                    </button>
                {/if}

                {#if isCreateMode && showAssetPicker}
                    <div class="picker-popup">
                        <input
                            class="picker-input"
                            type="text"
                            placeholder="Search {assetType}…"
                            bind:value={assetPickerQuery}
                            on:input={searchAssets}
                        />
                        <div class="picker-list">
                            {#each assetPickerResults as row (row.guid)}
                                <button class="picker-row" on:click={() => selectAsset(row)}>
                                    <span class="picker-primary">
                                        {#if row.icon}
                                            <img src={row.icon} alt="" class="picker-icon" />
                                        {/if}
                                    <span>{row.displayName || row.name}</span>
                                    </span>
                                    {#if hasCatalogPreexistingModifiers(row.guid)}
                                        <span class="picker-tag">Catalog Defaults</span>
                                    {/if}
                                </button>
                            {:else}
                                <div class="picker-empty">
                                    {assetPickerQuery ? "No results." : "Type to search."}
                                </div>
                            {/each}
                        </div>
                    </div>
                {/if}
            </section>

            <div class="row-fields">
                <section class="field-section">
                    <label class="field-label" for="item-level">Level</label>
                    <input
                        id="item-level"
                        class="num-input"
                        type="number"
                        min="1"
                        max="30"
                        bind:value={level}
                    />
                </section>

                <section class="field-section">
                    <label class="field-label" for="item-quality">Quality</label>
                    <select
                        id="item-quality"
                        class="field-select"
                        bind:value={rarity}
                        disabled={modifiersLocked}
                    >
                        {#each availableRarityIndexes as index}
                            <option value={index}>{RARITY_LABELS[index]}</option>
                        {/each}
                    </select>
                </section>

                {#if isEquipmentItem}
                    <section class="field-section">
                        <label class="field-label" for="item-durability">Durability</label>
                        <input
                            id="item-durability"
                            class="num-input"
                            type="number"
                            bind:value={durability}
                            disabled={isCreateMode}
                        />
                    </section>
                {/if}

                {#if isStackableItem}
                    <section class="field-section">
                        <label class="field-label" for="item-stack">Stack</label>
                        <input
                            id="item-stack"
                            class="num-input"
                            type="number"
                            min="1"
                            step="1"
                            bind:value={stackCount}
                        />
                    </section>
                {/if}
            </div>

            {#if isEquipmentItem}
                {#if traitName}
                    <section class="field-section">
                        <span class="field-label">Facet</span>
                        <div class="modifier-card facet-card">
                            <div class="modifier-header-line">
                                <span class="enchant-name">{traitDetail?.title || traitName}</span>
                                <span class="picker-tag facet-tag">Facet</span>
                            </div>
                            {#if traitDetail?.effectText}
                                <div class="modifier-effect">{traitDetail.effectText}</div>
                            {/if}
                        </div>
                    </section>
                {/if}

                {#if runeNames.length > 0}
                    <section class="field-section">
                        <span class="field-label">Runes</span>
                        <div class="enchant-list">
                            {#each currentRuneDetails as rune}
                                <div class="enchant-row static-row">
                                    <span class="enchant-name">{rune.displayName || rune.name}</span>
                                </div>
                            {/each}
                        </div>
                    </section>
                {/if}

                <section class="field-section">
                    <span class="field-label">Enchantments</span>
                    <div class="enchant-list">
                        {#each enchantments as entry, index}
                            {@const detail = enchantmentDetail(entry.guid)}
                            {@const rolledText = enchantmentRolledText(entry)}
                            {@const isRange = detail?.rollKind === "range" && detail.rollMin != null && detail.rollMax != null}
                            {@const rolledInt = isRange ? (computeRolledValue(entry.quality, detail!.rollKind, detail!.rollMin, detail!.rollMax, detail!.rollValue, detail!.rollUnit, detail!.rollIsNegative)?.rolledInt ?? detail!.rollMin!) : null}
                            <div class="enchant-row">
                                <div class="modifier-copy">
                                    <div class="modifier-header-line">
                                        <span class="enchant-name">{detail?.title || entry.name}</span>
                                        {#if detail?.isFacet}
                                            <span class="picker-tag facet-tag">Facet</span>
                                        {/if}
                                    </div>
                                    {#if rolledText}
                                        <div class="modifier-effect">{rolledText}</div>
                                    {:else if detail?.effectText}
                                        <div class="modifier-effect">{detail.effectText}</div>
                                    {/if}
                                    {#if isRange && !modifiersLocked}
                                        <div class="roll-control">
                                            <input
                                                type="range"
                                                class="roll-slider"
                                                min={detail!.rollMin!}
                                                max={detail!.rollMax!}
                                                step={1}
                                                value={rolledInt}
                                                on:input={(e) => setEnchantmentRoll(index, Number(e.currentTarget.value))}
                                            />
                                            <input
                                                type="number"
                                                class="roll-number"
                                                min={detail!.rollMin!}
                                                max={detail!.rollMax!}
                                                value={rolledInt}
                                                on:change={(e) => setEnchantmentRoll(index, Number(e.currentTarget.value))}
                                            />
                                        </div>
                                    {/if}
                                    {#if detail?.onlyOnItems && detail.onlyOnItems !== "Regular Item"}
                                        <div class="modifier-restriction">Only on: {detail.onlyOnItems}</div>
                                    {/if}
                                    {#if isFullySlotted && !modifiersLocked}
                                        <div class="exalt-control">
                                            <button
                                                class="exalt-btn"
                                                disabled={entry.exaltStacks <= 0}
                                                on:click={() => setExaltStacks(index, entry.exaltStacks - 1)}
                                                aria-label="Decrease exalt"
                                            >−</button>
                                            <span class="exalt-stars" aria-label="{entry.exaltStacks} exalt stacks">
                                                {#each Array(entry.exaltStacks) as _}★{/each}{#each Array(totalExaltStacks < MAX_EXALT_TOTAL && entry.exaltStacks < MAX_EXALT_TOTAL ? 1 : 0) as _}☆{/each}
                                            </span>
                                            <button
                                                class="exalt-btn"
                                                disabled={totalExaltStacks >= MAX_EXALT_TOTAL || entry.exaltStacks >= MAX_EXALT_TOTAL}
                                                on:click={() => setExaltStacks(index, entry.exaltStacks + 1)}
                                                aria-label="Increase exalt"
                                            >+</button>
                                        </div>
                                    {/if}
                                </div>
                                {#if !modifiersLocked}
                                    <div class="enchant-actions">
                                        <button class="move-btn" on:click={() => moveEnchantment(index, -1)} aria-label="Move enchantment up">↑</button>
                                        <button class="move-btn" on:click={() => moveEnchantment(index, 1)} aria-label="Move enchantment down">↓</button>
                                        <button class="remove-btn" on:click={() => removeEnchantment(index)} aria-label="Remove enchantment">✕</button>
                                    </div>
                                {/if}
                            </div>
                        {:else}
                            <div class="enchant-empty">None</div>
                        {/each}
                        <button class="small-btn" on:click={() => showEnchantSearch = true} disabled={modifiersLocked || !assetGuid || !isEquipmentItem}>
                            + Add Enchantment
                        </button>
                    </div>
                    {#if isFullySlotted && enchantments.length > 0 && !modifiersLocked}
                        <div class="exalt-total">
                            Exalt: {totalExaltStacks}/{MAX_EXALT_TOTAL}
                            {#if totalExaltStacks >= MAX_EXALT_TOTAL}
                                — <span class="exalt-label">EXALTED</span>
                            {/if}
                        </div>
                    {:else if enchantments.length > 0 && enchantments.length < enchantmentCap && !modifiersLocked}
                        <div class="field-hint">
                            Fill all {enchantmentCap} enchantment slots to enable exalting.
                        </div>
                    {/if}
                    {#if rarityWasAdjustedForEnchantments}
                        <div class="field-hint">
                            Rarity was adjusted to match the current enchantment count.
                        </div>
                    {/if}
                    {#if isRingItem}
                        <div class="field-hint">
                            Ring rarity and enchantments are fixed.
                        </div>
                    {:else if modifiersLocked}
                        <div class="field-hint">
                            Unique item defaults detected. Gold rarity and enchantments are locked.
                        </div>
                    {:else if catalogEntryMeta?.entryVersion !== "v2"}
                        <div class="field-hint">
                            This item falls back to broad compatibility because its equipment details are still v1-only.
                        </div>
                    {/if}
                    {#if isCreateMode}
                        <div class="field-hint">
                            New items always start at 100 durability.
                        </div>
                    {/if}
                </section>
            {/if}

            {#if saveError}
                <div class="save-error">{saveError}</div>
            {/if}
        </div>

        <div class="modal-footer">
            {#if mode === "edit"}
                <button class="btn delete-btn" on:click={removeItem}>
                    Delete
                </button>
            {/if}
            <button class="btn cancel-btn" on:click={close}>Cancel</button>
            <button class="btn save-btn" on:click={save}>
                {mode === "create" ? "Create" : "Save"}
            </button>
        </div>
    </div>
</div>

{#if showEnchantSearch && isEquipmentItem}
    <EnchantmentPicker
        {assetGuid}
        excludeGuids={enchantments.map(e => e.guid)}
        on:select={(e) => addEnchantment(e.detail)}
        on:close={() => showEnchantSearch = false}
    />
{/if}

<style>
    .modal-backdrop {
        position: fixed;
        inset: 0;
        background: rgba(0, 0, 0, 0.7);
        display: flex;
        align-items: center;
        justify-content: center;
        z-index: 500;
    }

    .modal {
        background: var(--bg-panel, #231a0e);
        border: 1px solid var(--border-color, #4a3520);
        border-radius: 10px;
        width: 520px;
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

    .close-btn,
    .remove-btn,
    .move-btn {
        background: none;
        border: none;
        color: var(--text-secondary, #a89070);
        font-size: 0.9em;
        padding: 2px 6px;
        border-radius: 4px;
        font-family: inherit;
    }

    .close-btn:hover,
    .remove-btn:hover,
    .move-btn:hover {
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

    .modal-footer {
        display: flex;
        justify-content: flex-end;
        gap: 8px;
        padding: 12px 16px;
        border-top: 1px solid var(--border-color, #4a3520);
    }

    .field-section {
        display: flex;
        flex-direction: column;
        gap: 6px;
    }

    .field-label {
        font-size: 0.72em;
        color: var(--accent-gold, #c8a050);
        text-transform: uppercase;
        letter-spacing: 0.06em;
        font-weight: 600;
        font-family: Georgia, serif;
    }

    .row-fields {
        display: grid;
        grid-template-columns: repeat(3, 1fr);
        gap: 16px;
    }

    .num-input,
    .field-select,
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

    .num-input {
        font-family: var(--font-mono);
        text-align: right;
    }

    .num-input:focus,
    .field-select:focus,
    .picker-input:focus {
        border-color: var(--accent-gold, #c8a050);
        outline: none;
    }

    .selected-asset,
    .enchant-row {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
        padding: 6px 8px;
        background: var(--bg-card, #2d2010);
        border: 1px solid var(--border-color, #4a3520);
        border-radius: 4px;
    }

    .selected-meta,
    .enchant-actions {
        display: flex;
        align-items: center;
        gap: 8px;
    }

    .modifier-copy {
        display: flex;
        flex-direction: column;
        gap: 4px;
        min-width: 0;
    }

    .selected-name,
    .enchant-name {
        font-size: 0.82em;
        color: var(--text-primary, #e8d5a3);
    }

    .picker-popup {
        display: flex;
        flex-direction: column;
        gap: 8px;
    }

    .picker-list {
        display: flex;
        flex-direction: column;
        gap: 4px;
        max-height: 220px;
        overflow-y: auto;
    }

    .picker-row,
    .small-btn,
    .btn {
        border: 1px solid var(--border-color, #4a3520);
        background: var(--bg-card, #2d2010);
        color: var(--text-primary, #e8d5a3);
        border-radius: 4px;
        padding: 6px 8px;
        font-family: inherit;
    }

    .picker-row:disabled,
    .small-btn:disabled,
    .btn:disabled,
    .field-select:disabled,
    .num-input:disabled {
        opacity: 0.65;
        cursor: not-allowed;
    }

    .picker-row {
        display: flex;
        align-items: center;
        gap: 8px;
        justify-content: space-between;
        text-align: left;
    }

    .picker-primary {
        display: flex;
        align-items: center;
        gap: 8px;
        min-width: 0;
    }

    .picker-tag {
        flex-shrink: 0;
        font-size: 0.72em;
        text-transform: uppercase;
        letter-spacing: 0.06em;
        color: var(--accent-gold, #c8a050);
    }

    .facet-tag {
        color: #7cc6ff;
    }

    .catalog-link,
    .modifier-effect,
    .modifier-restriction {
        font-size: 0.76em;
        color: var(--text-secondary, #a89070);
    }

    .catalog-link {
        color: var(--accent-gold, #c8a050);
        text-decoration: none;
    }

    .catalog-link:hover {
        text-decoration: underline;
    }

    .modifier-header-line {
        display: flex;
        align-items: center;
        gap: 8px;
        flex-wrap: wrap;
    }

    .static-row,
    .facet-card {
        align-items: flex-start;
    }

    .small-btn.primary,
    .save-btn {
        border-color: var(--accent-gold, #c8a050);
        color: var(--accent-gold, #c8a050);
    }

    .delete-btn {
        margin-right: auto;
        border-color: #9d3d3d;
        color: #ff8e8e;
        background: rgba(157, 61, 61, 0.2);
    }

    .picker-icon {
        width: 28px;
        height: 28px;
        object-fit: contain;
        flex-shrink: 0;
    }

    .enchant-list {
        display: flex;
        flex-direction: column;
        gap: 6px;
    }

    .enchant-empty,
    .picker-empty {
        font-size: 0.78em;
        color: var(--text-secondary, #a89070);
        padding: 6px 2px;
    }

    .roll-control {
        display: flex;
        align-items: center;
        gap: 8px;
        margin-top: 4px;
    }

    .roll-slider {
        flex: 1;
        accent-color: var(--accent-gold, #c8a050);
        cursor: pointer;
    }

    .roll-number {
        width: 54px;
        padding: 3px 6px;
        font-size: 0.82em;
        background: var(--bg-card, #2d2010);
        border: 1px solid var(--border-color, #4a3520);
        border-radius: 4px;
        color: var(--text-primary, #e8d5a3);
        font-family: var(--font-mono);
        text-align: right;
    }

    .roll-number:focus {
        border-color: var(--accent-gold, #c8a050);
        outline: none;
    }

    .field-hint {
        font-size: 0.78em;
        color: var(--text-secondary, #a89070);
        line-height: 1.35;
    }

    .exalt-control {
        display: flex;
        align-items: center;
        gap: 4px;
        margin-top: 4px;
    }

    .exalt-btn {
        background: var(--bg-card, #2d2010);
        border: 1px solid var(--border-color, #4a3520);
        color: var(--accent-gold, #c8a050);
        border-radius: 4px;
        width: 22px;
        height: 22px;
        font-size: 0.8em;
        padding: 0;
        line-height: 1;
        font-family: inherit;
    }

    .exalt-btn:disabled {
        opacity: 0.35;
        cursor: not-allowed;
    }

    .exalt-stars {
        font-size: 0.82em;
        color: var(--accent-gold, #c8a050);
        min-width: 20px;
    }

    .exalt-total {
        font-size: 0.78em;
        color: var(--text-secondary, #a89070);
        margin-top: 4px;
    }

    .exalt-label {
        color: var(--accent-gold, #c8a050);
        font-weight: 600;
        text-transform: uppercase;
        letter-spacing: 0.06em;
    }

    .save-error {
        font-size: 0.78em;
        color: #ffb0b0;
    }
</style>

<script lang="ts">
    import {
        editStash,
        getRootPrimitives,
        type DocMeta,
    } from "./bridge";

    export let docs: DocMeta[];

    const STAT_KEYS = [
        { key: "Level",   label: "Level" },
        { key: "XP",      label: "XP" },
        { key: "Health",  label: "Health" },
        { key: "Stamina", label: "Stamina" },
        { key: "Gold",    label: "Gold" },
    ];

    const ATTR_KEYS = [
        { key: "Strength",     label: "Strength" },
        { key: "Dexterity",    label: "Dexterity" },
        { key: "Intelligence", label: "Intelligence" },
        { key: "Faith",        label: "Faith" },
        { key: "Focus",        label: "Focus" },
        { key: "Load",         label: "Load" },
    ];

    interface FieldEntry { key: string; label: string; value: number }

    // Original values loaded from WASM — never mutated after load
    let originalStats: FieldEntry[] = STAT_KEYS.map(k => ({ ...k, value: 0 }));
    let originalAttributes: FieldEntry[] = ATTR_KEYS.map(k => ({ ...k, value: 0 }));
    let loading = true;
    let lastLoadedDocIndex: number | undefined;

    $: metaDoc = docs.find(d => d.rootType === "Quantum.CharacterMetadata");
    $: metaDocIdx = metaDoc?.index ?? -1;

    $: if (metaDoc !== undefined && metaDoc.index !== lastLoadedDocIndex) {
        lastLoadedDocIndex = metaDoc.index;
        loadStats(metaDoc);
    }

    // Derive displayed values from originals + editStash, so reset is automatic
    $: stats = originalStats.map(s => {
        const v = $editStash[metaDocIdx]?.[s.key];
        return v !== undefined ? { ...s, value: parseInt(v, 10) } : s;
    });
    $: attributes = originalAttributes.map(a => {
        const v = $editStash[metaDocIdx]?.[a.key];
        return v !== undefined ? { ...a, value: parseInt(v, 10) } : a;
    });

    async function loadStats(doc: DocMeta) {
        loading = true;
        const primitives = await getRootPrimitives(doc.index);
        const map: Record<string, number> = {};
        for (const p of primitives) map[p.path] = parseFloat(p.value);

        originalStats = STAT_KEYS.map(k => ({ ...k, value: Math.round(map[k.key] ?? 0) }));
        originalAttributes = ATTR_KEYS.map(k => ({ ...k, value: Math.round(map[k.key] ?? 0) }));
        loading = false;
    }

    function onFieldChange(key: string, rawValue: string) {
        const num = parseInt(rawValue, 10);
        if (isNaN(num) || !metaDoc) return;
        const docIdx = metaDoc.index;

        // Write to global edit stash — reactive $: stats/$: attributes derive the display
        editStash.update(stash => ({
            ...stash,
            [docIdx]: { ...(stash[docIdx] ?? {}), [key]: String(num) },
        }));
    }
</script>

<div class="stats-panel">
    {#if loading}
        <div class="loading">Loading…</div>
    {:else}
        <section class="stat-section">
            <h3 class="section-label">Stats</h3>
            {#each stats as field (field.key)}
                <div class="field-row">
                    <span class="field-label">{field.label}</span>
                    <input
                        class="field-input"
                        type="number"
                        data-field={field.key}
                        value={field.value}
                        on:change={e => onFieldChange(field.key, e.currentTarget.value)}
                    />
                </div>
            {/each}
        </section>

        <section class="stat-section">
            <h3 class="section-label">Attributes</h3>
            {#each attributes as field (field.key)}
                <div class="field-row">
                    <span class="field-label">{field.label}</span>
                    <input
                        class="field-input"
                        type="number"
                        data-field={field.key}
                        value={field.value}
                        on:change={e => onFieldChange(field.key, e.currentTarget.value)}
                    />
                </div>
            {/each}
        </section>
    {/if}
</div>

<style>
    .stats-panel {
        padding: 16px 12px;
        box-sizing: border-box;
    }

    .loading {
        color: var(--text-secondary, #a89070);
        font-size: 0.85em;
        padding: 8px;
    }

    .stat-section {
        margin-bottom: 20px;
    }

    .section-label {
        font-family: Georgia, serif;
        font-size: 0.72em;
        font-weight: 600;
        color: var(--accent-gold, #c8a050);
        text-transform: uppercase;
        letter-spacing: 0.08em;
        margin: 0 0 10px;
        padding-bottom: 4px;
        border-bottom: 1px solid var(--border-color, #4a3520);
    }

    .field-row {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 4px 0;
        gap: 8px;
    }

    .field-label {
        font-size: 0.8em;
        color: var(--text-secondary, #a89070);
        flex: 1;
        min-width: 0;
    }

    .field-input {
        width: 64px;
        padding: 3px 6px;
        font-size: 0.82em;
        text-align: right;
        background: var(--bg-card, #2d2010);
        border: 1px solid var(--border-color, #4a3520);
        border-radius: 4px;
        color: var(--text-primary, #e8d5a3);
        font-family: var(--font-mono);
        flex-shrink: 0;
        transition: border-color 0.15s;
    }

    .field-input:focus {
        border-color: var(--accent-gold, #c8a050);
        outline: none;
    }
</style>

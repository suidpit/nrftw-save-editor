<script lang="ts">
    import { importAppearanceFromBytes, type DocMeta } from "./bridge";

    export let docs: DocMeta[];

    $: metaDoc = docs.find((d) => d.rootType === "Quantum.CharacterMetadata");
    $: metaDocIdx = metaDoc?.index ?? 0;

    let fileInput: HTMLInputElement;
    let status: string | null = null;
    let busy = false;

    async function onFileChange(e: Event) {
        const file = (e.target as HTMLInputElement).files?.[0];
        if (!file) return;
        busy = true;
        status = null;
        try {
            const bytes = new Uint8Array(await file.arrayBuffer());
            const count = await importAppearanceFromBytes(bytes, metaDocIdx);
            status =
                count > 0
                    ? `Imported ${count} fields from ${file.name}`
                    : `No customization found in ${file.name}`;
        } catch (err) {
            status = `Error: ${err instanceof Error ? err.message : String(err)}`;
        } finally {
            busy = false;
            fileInput.value = "";
        }
    }
</script>

<div class="appearance-panel">
    <h3 class="section-label">Appearance</h3>
    <input
        bind:this={fileInput}
        type="file"
        accept=".cerimal"
        class="file-input"
        on:change={onFileChange}
    />
    <button
        class="import-btn"
        disabled={busy}
        on:click={() => fileInput.click()}
    >
        {busy ? "Importing…" : "Import from another character…"}
    </button>
    {#if status}
        <div class="appearance-status">{status}</div>
    {/if}
</div>

<style>
    .appearance-panel {
        padding: 0 12px 16px;
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

    .file-input {
        display: none;
    }

    .import-btn {
        width: 100%;
        padding: 6px 10px;
        font-size: 0.78em;
        background: var(--bg-card, #2d2010);
        border: 1px solid var(--border-color, #4a3520);
        border-radius: 4px;
        color: var(--text-primary, #e8d5a3);
        cursor: pointer;
        transition: border-color 0.15s;
        text-align: center;
    }

    .import-btn:hover:not(:disabled) {
        border-color: var(--accent-gold, #c8a050);
    }

    .import-btn:disabled {
        opacity: 0.5;
        cursor: default;
    }

    .appearance-status {
        margin-top: 8px;
        font-size: 0.75em;
        color: var(--text-secondary, #a89070);
        word-break: break-all;
    }
</style>

<script lang="ts">
    import { createEventDispatcher } from "svelte";
    import type { BootStatus } from "./bridge";

    export let bootStatus: BootStatus;
    export let parsing: boolean = false;
    export let parseError: string = "";

    const dispatch = createEventDispatcher<{ file: File }>();

    let dragging = false;

    function onFileInput(e: Event) {
        const input = e.target as HTMLInputElement;
        if (input.files?.[0]) dispatch("file", input.files[0]);
    }

    function onDrop(e: DragEvent) {
        e.preventDefault();
        dragging = false;
        const file = e.dataTransfer?.files?.[0];
        if (file) dispatch("file", file);
    }

    function onDragOver(e: DragEvent) {
        e.preventDefault();
        dragging = true;
    }

    function onDragLeave() {
        dragging = false;
    }
</script>

<div class="landing-page">
    <div class="landing-inner">
        <div class="landing-brand">
            <div class="landing-logo-frame">
                <img
                    class="landing-logo"
                    src="/logo.png"
                    alt="No Rest for the Wicked Save Editor logo"
                />
            </div>
        </div>

        <div class="landing-content">
            <header class="landing-header">
                <h1 class="title">No Rest for the Wicked Save Editor</h1>
            </header>

            {#if parseError}
                <div class="parse-error" role="alert" aria-live="assertive">
                    <strong>{parseError}</strong>
                </div>
            {/if}

            <div
                class="disclaimer"
                role="note"
                aria-label="Client-side only notice"
            >
                <span class="disclaimer-label">Client-side only!</span>
                <p class="disclaimer-text">
                    This editor runs entirely in the browser and does not
                    transmit any of your data to any backend server.
                </p>
            </div>

            <div
                class="drop-zone"
                class:dragging
                on:drop={onDrop}
                on:dragover={onDragOver}
                on:dragleave={onDragLeave}
                role="region"
                aria-label="Save file upload"
            >
                <div class="drop-zone-inner">
                    {#if parsing}
                        <p class="drop-hint">Parsing save file…</p>
                    {:else if bootStatus.phase === "loading_wasm"}
                        <p class="drop-hint loading-hint">Loading engine…</p>
                    {:else if bootStatus.phase === "error"}
                        <p class="drop-error">⚠ {bootStatus.message}</p>
                    {:else}
                        <p class="drop-text">Drag'n Drop a .cerimal save</p>
                        <p class="drop-or">— or —</p>
                        <label class="upload-btn">
                            Browse files
                            <input
                                type="file"
                                accept=".cerimal"
                                on:change={onFileInput}
                                style="display:none"
                            />
                        </label>
                    {/if}
                </div>
            </div>

            <div class="instructions">
                <h3 class="instructions-title">How to use</h3>
                <ol class="instructions-list">
                    <li>
                        Locate your save file at <code class="path-inline"
                            >%APPDATA%\LocalLow\Moon Studios\No Rest For The
                            Wicked\Datastore\$build_id</code
                        >
                    </li>
                    <li>Upload the <code>.cerimal</code> file above</li>
                    <li>Edit your character stats, equipment, and inventory</li>
                    <li>Download the patched save and replace the original</li>
                </ol>
            </div>
        </div>
    </div>
</div>

<style>
    .landing-page {
        height: 100%;
        background:
            linear-gradient(180deg, rgba(20, 7, 4, 0.74), rgba(20, 7, 4, 0.88)),
            radial-gradient(
                circle at top,
                rgba(124, 28, 13, 0.24),
                rgba(20, 7, 4, 0.06) 36%
            ),
            url("/nrftw-bg.jpg") center top / cover no-repeat,
            var(--bg-primary, #1a1209);
        display: flex;
        align-items: flex-start;
        justify-content: center;
        padding: 24px;
        box-sizing: border-box;
        overflow-y: auto;
    }

    .landing-inner {
        width: 100%;
        max-width: 1080px;
        display: flex;
        flex-direction: column;
        gap: 32px;
        margin: 0 auto;
    }

    .landing-brand {
        display: flex;
        justify-content: center;
    }

    .landing-content {
        display: flex;
        flex-direction: column;
        gap: 32px;
    }

    .landing-header {
        text-align: center;
    }

    .landing-logo {
        display: block;
        width: 100%;
        height: 100%;
        object-fit: cover;
    }

    .landing-logo-frame {
        width: min(100%, 680px);
        aspect-ratio: 1235 / 815;
        margin: 0 auto;
        overflow: hidden;
    }

    .title {
        font-family: Georgia, "Times New Roman", serif;
        font-size: 2em;
        color: var(--accent-gold, #c8a050);
        margin: 0 0 4px;
        letter-spacing: 0.04em;
        text-shadow: 0 2px 8px rgba(200, 160, 80, 0.3);
        text-align: center;
    }

    .disclaimer {
        display: flex;
        flex-direction: column;
        gap: 6px;
        margin: 0;
        padding: 14px 16px;
        border: 1px solid rgba(104, 180, 216, 0.42);
        border-radius: 8px;
        background: rgba(39, 80, 108, 0.28);
        box-shadow:
            0 0 0 1px rgba(104, 180, 216, 0.08),
            0 0 24px rgba(72, 156, 214, 0.14);
        color: #d9efff;
        text-align: left;
    }

    .disclaimer-label {
        font-size: 0.74em;
        font-weight: 700;
        letter-spacing: 0.08em;
        text-transform: uppercase;
        text-align: center;
        color: #9ed8ff;
    }

    .disclaimer-text {
        margin: 0;
        font-size: 0.92em;
        line-height: 1.5;
    }

    .drop-zone {
        border: 2px dashed var(--border-color, #4a3520);
        border-radius: 12px;
        background: var(--bg-panel, #231a0e);
        transition:
            border-color 0.2s,
            background 0.2s;
        cursor: pointer;
    }

    .drop-zone.dragging {
        border-color: var(--accent-gold, #c8a050);
        background: #2d2010;
    }

    .drop-zone-inner {
        padding: 48px 32px;
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 12px;
    }

    .drop-text {
        font-family: Georgia, serif;
        font-size: 1.15em;
        color: var(--text-primary, #e8d5a3);
        margin: 0;
    }

    .drop-or {
        font-size: 0.82em;
        color: var(--text-secondary, #a89070);
        margin: 0;
    }

    .drop-hint {
        font-size: 0.9em;
        color: var(--text-secondary, #a89070);
        margin: 0;
    }

    .loading-hint {
        animation: pulse 1.5s ease-in-out infinite;
    }

    @keyframes pulse {
        0%,
        100% {
            opacity: 0.6;
        }
        50% {
            opacity: 1;
        }
    }

    .drop-error {
        font-size: 0.85em;
        color: #f88;
        margin: 0;
    }

    .upload-btn {
        display: inline-block;
        padding: 9px 24px;
        background: transparent;
        border: 1px solid var(--accent-gold, #c8a050);
        border-radius: 6px;
        color: var(--accent-gold, #c8a050);
        font-size: 0.88em;
        cursor: pointer;
        letter-spacing: 0.05em;
        transition:
            background 0.15s,
            color 0.15s;
        font-family: inherit;
    }

    .upload-btn:hover {
        background: var(--accent-gold, #c8a050);
        color: #1a1209;
    }

    .parse-error {
        padding: 10px 16px;
        background: rgba(139, 0, 0, 0.15);
        border: 1px solid rgba(139, 0, 0, 0.4);
        border-radius: 6px;
        color: #f88;
        font-size: 0.85em;
    }

    .instructions {
        padding: 20px 24px;
        background: var(--bg-panel, #231a0e);
        border: 1px solid var(--border-color, #4a3520);
        border-radius: 8px;
    }

    .instructions-title {
        font-family: Georgia, serif;
        font-size: 0.85em;
        color: var(--text-secondary, #a89070);
        text-transform: uppercase;
        letter-spacing: 0.08em;
        margin: 0 0 12px;
    }

    .instructions-list {
        margin: 0;
        padding: 0 0 0 18px;
        display: flex;
        flex-direction: column;
        gap: 6px;
    }

    .instructions-list li {
        font-size: 0.82em;
        color: var(--text-secondary, #a89070);
        line-height: 1.5;
    }

    code {
        background: var(--bg-card, #2d2010);
        border: 1px solid var(--border-color, #4a3520);
        padding: 1px 5px;
        border-radius: 3px;
        font-family: var(--font-mono);
        font-size: 0.9em;
        color: var(--text-primary, #e8d5a3);
    }

    @media (min-height: 900px) {
        .landing-page {
            align-items: center;
        }
    }

    @media (min-width: 900px) {
        .landing-inner {
            display: grid;
            grid-template-columns: minmax(280px, 380px) minmax(0, 1fr);
            align-items: center;
            gap: 56px;
        }

        .landing-brand {
            align-self: center;
        }

        .landing-header {
            text-align: left;
        }
    }

    .path-inline {
        white-space: nowrap;
        display: inline-block;
        max-width: 100%;
        overflow-x: auto;
        vertical-align: bottom;
    }
</style>

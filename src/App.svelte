<script lang="ts">
    import { onMount } from "svelte";
    import {
        catalogTarget,
        initBridge,
        onBootStatus,
        parseSaveFile,
        resetPendingChanges,
        type DocMeta,
        type BootStatus,
    } from "./lib/bridge";
    import LandingPage from "./lib/LandingPage.svelte";
    import MainLayout from "./lib/MainLayout.svelte";

    let bootStatus: BootStatus = { phase: "idle" };
    let docs: DocMeta[] = [];
    let fileName = "";
    let parseError = "";
    let parsing = false;

    onMount(() => {
        const unsub = onBootStatus((s) => (bootStatus = s));
        initBridge();
        return unsub;
    });

    async function handleFile(file: File) {
        if (bootStatus.phase !== "ready") {
            parseError = "WASM not ready yet — please wait.";
            return;
        }
        parsing = true;
        parseError = "";
        fileName = file.name;
        try {
            const buf = await file.arrayBuffer();
            docs = await parseSaveFile(new Uint8Array(buf));
        } catch (e) {
            parseError =
                "Could not parse the save file. Is it a valid .cerimal?";
            docs = [];
        } finally {
            parsing = false;
        }
    }

    function handleResetFile() {
        docs = [];
        fileName = "";
        parseError = "";
        resetPendingChanges();
        catalogTarget.set(null);
    }
</script>

<!-- Boot status indicator — always present so tests can locate it -->
<div
    class="boot-status"
    class:ready={bootStatus.phase === "ready"}
    class:error={bootStatus.phase === "error"}
    aria-live="polite"
>
    {#if bootStatus.phase === "error"}
        Error: {bootStatus.message}
    {:else if bootStatus.phase === "loading_wasm"}
        Loading engine…
    {:else if bootStatus.phase === "ready"}
        Ready
    {/if}
</div>

{#if docs.length > 0}
    <MainLayout {docs} {fileName} on:resetfile={handleResetFile} />
{:else}
    <LandingPage
        {bootStatus}
        {parsing}
        {parseError}
        on:file={(e) => handleFile(e.detail)}
    />
{/if}

<style>
    :global(*) {
        box-sizing: border-box;
    }

    :global(:root) {
        --bg-primary: #1a1209;
        --bg-panel: #231a0e;
        --bg-card: #2d2010;
        --border-color: #4a3520;
        --accent-gold: #c8a050;
        --accent-red: #8b0000;
        --text-primary: #e8d5a3;
        --text-secondary: #a89070;
        --rarity-white: #c8c8c8;
        --rarity-blue: #4488ff;
        --rarity-purple: #aa44ff;
        --rarity-gold: #ffaa00;
    }

    :global(body) {
        margin: 0;
        padding: 0;
        background: var(--bg-primary);
        color: var(--text-primary);
        font-family:
            system-ui,
            -apple-system,
            sans-serif;
    }

    :global(button) {
        cursor: pointer;
    }

    .boot-status {
        position: fixed;
        top: 6px;
        right: 10px;
        font-size: 0.68em;
        color: var(--text-secondary, #a89070);
        z-index: 9999;
        pointer-events: none;
        font-family: "Cascadia Code", "Fira Mono", monospace;
        transition: opacity 0.5s;
    }

    .boot-status.ready {
        /* Still in DOM but subtle — needed for Playwright .waitFor() */
        color: #3a2a10;
        opacity: 0.6;
    }

    .boot-status.error {
        color: #f88;
        opacity: 1;
    }
</style>

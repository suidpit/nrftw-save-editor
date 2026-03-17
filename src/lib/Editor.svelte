<script lang="ts">
    import type { DocMeta } from "./bridge";
    import StatsPanel from "./StatsPanel.svelte";
    import InventoryGrid from "./InventoryGrid.svelte";

    export let docs: DocMeta[];
</script>

<div class="editor-layout">
    <div class="column stats-col">
        <StatsPanel {docs} />
    </div>
    <div class="column inventory-col">
        <InventoryGrid {docs} />
    </div>
</div>

<style>
    .editor-layout {
        display: grid;
        grid-template-columns: 220px 1fr;
        height: 100%;
        overflow: hidden;
    }

    .column {
        overflow: hidden;
        height: 100%;
        display: flex;
        flex-direction: column;
    }

    .stats-col {
        border-right: 1px solid var(--border-color, #4a3520);
    }
</style>

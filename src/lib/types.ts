export interface ItemEditorDraft {
    docIdx: number;
    itemPath: string | null;
    assetGuid: string;
    assetType: string;
    displayName: string;
    icon: string | null;
    level: number;
    rarityNum: number;
    durability: number;
    stackCount: number;
    runeGuids: string[];
    runeNames: string[];
    enchantmentGuids: string[];
    enchantmentNames: string[];
    traitGuid: string;
    traitName: string;
    isNew: boolean;
    draftId?: string;
}

export interface InventoryViewItem extends ItemEditorDraft {
    index: number;
    slotNum: number;
    rarityClass: "white" | "blue" | "purple" | "gold";
}

export interface PendingItemDelete {
    docIdx: number;
    itemPath: string;
}

export interface PendingItemChanges {
    edits: Record<string, ItemEditorDraft>;
    creates: ItemEditorDraft[];
    deletes: PendingItemDelete[];
}

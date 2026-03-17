// Game rarity tiers — mirrored from mutations.rs
export const RARITY_NAMES = ["white", "blue", "purple", "gold"] as const;
export const RARITY_LABELS = ["Normal", "Rare", "Cursed", "Unique"] as const;
export const RARITY_COLORS: Record<string, string> = {
    white: "var(--rarity-white, #c8c8c8)",
    blue: "var(--rarity-blue, #4488ff)",
    purple: "var(--rarity-purple, #aa44ff)",
    gold: "var(--rarity-gold, #ffaa00)",
} as const;

export const INVENTORY_ASSET_TYPES = [
    { value: "WeaponStaticDataAsset", label: "Weapon" },
    { value: "HelmDataAsset", label: "Helmet" },
    { value: "BodyDataAsset", label: "Body Armor" },
    { value: "PantsDataAsset", label: "Pants" },
    { value: "GlovesDataAsset", label: "Gloves" },
    { value: "RingsDataAsset", label: "Ring" },
    { value: "GenericItemDataAsset", label: "Generic" },
    { value: "QuickItemDataAsset", label: "Quick Item" },
    { value: "FoodItemDataAsset", label: "Food" },
    { value: "GenericToolItemDataAsset", label: "Tool" },
    { value: "ThrowableItemDataAsset", label: "Throwable" },
    { value: "EnchantGemItemDataAsset", label: "Enchant Gem" },
    { value: "EquipmentManipulationItemDataAsset", label: "Equipment Manipulation" },
    { value: "FuelItemDataAsset", label: "Fuel" },
    { value: "HeroRuneDataAsset", label: "Hero Rune" },
] as const;

export const INVENTORY_ASSET_TYPE_VALUES: ReadonlySet<string> = new Set(
    INVENTORY_ASSET_TYPES.map((assetType) => assetType.value),
);

export const EQUIPMENT_ASSET_TYPE_VALUES: ReadonlySet<string> = new Set([
    "WeaponStaticDataAsset",
    "HelmDataAsset",
    "BodyDataAsset",
    "PantsDataAsset",
    "GlovesDataAsset",
    "RingsDataAsset",
]);

export const STACKABLE_ASSET_TYPE_VALUES: ReadonlySet<string> = new Set(
    INVENTORY_ASSET_TYPES.map((assetType) => assetType.value).filter(
        (assetType) => !EQUIPMENT_ASSET_TYPE_VALUES.has(assetType),
    ),
);

export function isInventoryAssetType(assetType: string | null | undefined): boolean {
    return assetType !== null && assetType !== undefined
        ? INVENTORY_ASSET_TYPE_VALUES.has(assetType)
        : false;
}

export function isEquipmentAssetType(assetType: string | null | undefined): boolean {
    return assetType !== null && assetType !== undefined
        ? EQUIPMENT_ASSET_TYPE_VALUES.has(assetType)
        : false;
}

export function isStackableAssetType(assetType: string | null | undefined): boolean {
    return assetType !== null && assetType !== undefined
        ? STACKABLE_ASSET_TYPE_VALUES.has(assetType)
        : false;
}

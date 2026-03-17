import uniqueItemData from "./data/all_unique_item_enchantments.json";

export interface UniqueItemPresetEnchantment {
    guid: string;
    name: string;
    displayName: string;
}

export interface UniqueItemPreset {
    assetGuid: string;
    assetName: string;
    assetDisplayName: string;
    enchantments: UniqueItemPresetEnchantment[];
}

interface UniqueItemDataPayload {
    items?: Array<{
        item?: {
            guid?: string;
            name?: string;
            displayName?: string;
        };
        enchantments?: Array<{
            guid?: string;
            name?: string;
            displayName?: string;
        }>;
    }>;
}

const payload = uniqueItemData as UniqueItemDataPayload;

const presetEntries = (payload.items ?? [])
    .map((entry): UniqueItemPreset | null => {
        const assetGuid = entry.item?.guid;
        if (!assetGuid) return null;

        return {
            assetGuid,
            assetName: entry.item?.name ?? assetGuid,
            assetDisplayName:
                entry.item?.displayName ?? entry.item?.name ?? assetGuid,
            enchantments: (entry.enchantments ?? [])
                .filter(
                    (enchantment): enchantment is Required<typeof enchantment> =>
                        Boolean(enchantment.guid),
                )
                .map((enchantment) => ({
                    guid: enchantment.guid,
                    name: enchantment.name ?? enchantment.guid,
                    displayName:
                        enchantment.displayName ??
                        enchantment.name ??
                        enchantment.guid,
                })),
        };
    })
    .filter((entry): entry is UniqueItemPreset => entry !== null);

const presetByAssetGuid = new Map(
    presetEntries.map((preset) => [preset.assetGuid, preset] as const),
);

export function getUniqueItemPreset(assetGuid: string): UniqueItemPreset | null {
    return presetByAssetGuid.get(assetGuid) ?? null;
}

export function isKnownUniqueItem(assetGuid: string): boolean {
    return presetByAssetGuid.has(assetGuid);
}

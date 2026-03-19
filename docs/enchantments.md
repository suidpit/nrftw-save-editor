# Enchantments

## Save shape

An item's enchantment block stores three distinct things for each enchantment:

- `Asset.Id`: which enchantment this is
- `ExaltStacks`: exalt / upgrade state
- `Quality`: the roll for this specific enchantment instance

The trait uses the same structure.

Conceptually, an enchanted item stores:

```text
Enchantment {
  Asset.Id
  ExaltStacks
  Quality
}
```

## What `Quality` means

`Quality` does not identify the enchantment, and it does not contain the enchantment's min/max range.

It is the roll parameter for that enchantment instance: the value the game uses to pick a concrete number from the enchantment's allowed range.

Example:

- enchantment: `Lightning Damage Increased`
- allowed range: `3% - 10%`
- stored `Quality`: the raw encoded roll
- displayed value: for example `7%`

So:

- `Asset.Id` says what the enchantment does
- `Quality` says where this item landed inside the allowed range

## How `Quality` is encoded

`Quality` is stored as a `ulong` / `u64`.

The game decodes it with:

```text
QUALITY_DIVISOR = 0xFFFF00010000
QUALITY_FIXED_ONE = 65536

fixed = Quality / QUALITY_DIVISOR
normalized = fixed / QUALITY_FIXED_ONE
```

Where `normalized` is clamped to `0..1`.

The editor also exposes the same value as a rounded percent:

```text
quality_percent = round(normalized * 100)
```

## `q` and `1 - q`

After decoding `Quality`, the game gets a normalized roll parameter `q` in the `0..1` range.

When an enchantment has a numeric range, the displayed value is produced by interpolating between the minimum and maximum values. The interpolation parameter is not always `q` directly. Depending on the enchantment's scaling semantics, the game may use either:

```text
t = q
```

or:

```text
t = 1 - q
```

Conceptually:

```text
rolled_value = interpolate(min, max, t)
```

This allows the save to store one notion of roll quality while still supporting enchantments whose "better" outcome lies at opposite ends of their displayed numeric range.

## How the game rounds the rolled value

The game does **not** jump straight from:

```text
rolled_value = interpolate(min, max, t)
```

to the final integer shown in the item details.

There are two relevant stages:

1. modifier-space rounding
2. tooltip preview rounding / formatting

### 1. Modifier-space rounding

For numeric modifiers, the interpolation path goes through:

```text
Quantum::ScaledModifier::GetProbe
```

That function computes the interpolated fixed-point value and then immediately calls:

```text
Quantum::ScaledModifier::Round
```

Which delegates to:

```text
Quantum::MoonFPMath::Round(value, decimalPlaces)
```

Where `decimalPlaces` comes from the modifier type's `get_RoundingPrecision()`.

So the internal rolled value is:

```text
internal_value = Round(interpolate(...), precision)
```

Important detail:

- `MoonFPMath::Round(..., 0)` uses **banker's rounding** / round-half-to-even
- higher precisions round to that many decimal places in fixed-point

For `StatModifier` / `ItemStatModifier`, the precision comes from:

```text
Quantum::StatTypeExtension::GetRoundPrecision
```

Observed rules:

- some stats use `1`
- `HealthRegen` / `FocusRegen` use `2`
- most stat/item-stat modifiers relevant to enchantments use `2`
- plain base-stat values with no special class/modification can use `0`

### 2. Tooltip preview rounding

The integer shown in the inventory item details usually comes from a later preview layer, not directly from the raw modifier value.

The item tooltip path for stat-based enchantments goes through:

```text
ItemStatModifierDescriptionExtension::ExtractDescriptionData
StatModifierDescriptionExtension::ExtractDescriptionData
StatPreviewExtenstion::GetStatModificationValuePreview
```

That preview code takes the already-rounded modifier FP value, applies the stat preview multiplier, and then rounds again with:

```text
(value * preview_multiplier + 0x8000) >> 16
```

For positive magnitudes, that is round-to-nearest with `.5` going **up**.

The sign is applied afterwards, so visible negative values are effectively rounded by magnitude first, then signed.

In other words, the item-detail integer is usually:

```text
display_value = round_half_up(abs(internal_value * preview_multiplier))
```

with the sign added back later if needed.

## Practical takeaway for reproducing the in-game integer

If you want to match the number shown in the in-game item details for typical stat-based enchantments:

1. decode `Quality` to `q`
2. choose `t = q` or `t = 1 - q`
3. interpolate in fixed-point
4. apply `ScaledModifier::Round` using the modifier's rounding precision
5. apply the stat preview multiplier
6. round the preview value to nearest integer with `.5` rounded up

So the tooltip integer is **not** just a plain truncation of the interpolated value.
It is a display-space rounded preview of an already-rounded modifier value.

## What is needed to compute the rolled value

To reconstruct the number shown in a tooltip, you need both:

1. the enchantment identity
2. the enchantment quality

Why:

- the enchantment identity gives the roll metadata: whether it is a range, a fixed value, or a special effect
- `Quality` gives the position used for that specific item

`Quality` alone is not enough, because it does not include the range.

## Catalog-side roll metadata

For catalog purposes, enchantments are normalized into three buckets:

- `range`
- `fixed`
- `special`

The catalog DB stores the parsed roll metadata alongside the enchantment row:

- `roll_kind` — "range", "fixed", or "special"
- `roll_min`, `roll_max` — range bounds (range only)
- `roll_value` — single value (fixed only)
- `roll_unit` — "%" or "flat"
- `roll_is_negative` — 1 if detrimental to player
- `roll_text` — display template with `{value}` placeholder for ranges; corrected version of `effect_text` with accurate durations and wording from `description_text`

This lets the editor combine:

- save-side `Quality`
- catalog-side roll metadata

to reconstruct rolled enchantment values.

## Roll classification rules

Each enchantment's `effect_text` is classified into one of three kinds:

| Kind | Criteria | Populated fields |
|------|----------|-----------------|
| `range` | Contains `( N - M )` pattern | `roll_min`, `roll_max`, `roll_unit` |
| `fixed` | Contains a single numeric value | `roll_value`, `roll_unit` |
| `special` | No numeric value | None (all NULL) |

`roll_unit` is `"%"` when the value has a percent suffix, `"flat"` otherwise.

## Negative (plagued) enchantments

`roll_is_negative = 1` marks enchantments that are detrimental to the player. For these, a higher Quality means a *lesser* penalty (the roll interpolation is inverted).

Negative indicators:
- "Damage Taken increased by", "Attack Stamina Cost increased by"
- "Damage Resistance reduced by", "Healing reduced by", "Max Focus reduced by", "Movement Speed reduced by", "Attack Damage reduced by"
- "Lose N Stamina/Focus", "Drain Health/Focus"
- "Block disabled", "Parry disabled", "Heavy Roll only", "Unrepairable"

Beneficial reductions (NOT negative):
- "Focus Cost reduced by", "Stamina Cost reduced by", "Weight reduced by"

## Mental model

An enchantment on an item is not just "this modifier exists".

It is:

- which modifier it is
- how good that particular roll was
- how many exalt stacks it has

That is why two items with the same enchantment can show different numbers in-game even though they share the same enchantment asset.

## Reverse engineering notes

Useful call path for item-detail enchantments:

```text
EnchantmentElement::EnchantmentConfig::Populate
  -> EnchantmentDescriptionExtension::GetDescription
  -> DescriptionDataProviderExtensions::ResolveDescription
  -> ...DescriptionExtension::ExtractDescriptionData
```

Most relevant functions found so far:

- `Moon::Forsaken::EnchantmentElement::EnchantmentConfig::Populate` at `0x188cb1c00`
- `Moon::Forsaken::EnchantmentDescriptionExtension::GetDescription` at `0x188945eb0`
- `Quantum::ScaledModifier::GetProbe` at `0x185c200b0`
- `Quantum::ScaledModifier::Round` at `0x185c203b0`
- `Quantum::MoonFPMath::Round` at `0x185cd1a20`
- `Quantum::StatTypeExtension::GetRoundPrecision` at `0x185c6c540`
- `Moon::Forsaken::ItemStatModifierDescriptionExtension::ExtractDescriptionData` at `0x188949ea0`
- `Moon::Forsaken::StatModifierDescriptionExtension::ExtractDescriptionData` at `0x18894c370`
- `Moon::Forsaken::StatPreviewExtenstion::GetStatModificationValuePreview` at `0x188ea26e0`

What to look for next when reversing new tooltip cases:

- whether the enchantment resolves to `StatModifier`, `ItemStatModifier`, or another description extension
- the modifier's `get_RoundingPrecision()` implementation
- whether the final text comes from `DescriptionPacket::ToString` or from a prebuilt preview string
- whether the preview path applies an extra multiplier before formatting

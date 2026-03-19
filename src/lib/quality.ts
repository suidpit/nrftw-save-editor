/**
 * quality.ts — encode/decode the Quality u64 field stored per enchantment.
 *
 * Quality is a u64 value that encodes a FP16.16 fixed-point number in [0, 1].
 * The codec:
 *   decode: fixed = Quality / DIVISOR;  normalized = fixed / 65536
 *   encode: Quality = round(normalized * 65536) * DIVISOR
 *
 * DIVISOR = 0xFFFF00010000 (= 281470681808896) — exceeds Number.MAX_SAFE_INTEGER,
 * so all arithmetic on the raw u64 string uses BigInt.
 */

const QUALITY_DIVISOR = BigInt("281470681808896"); // 0xFFFF00010000
const QUALITY_FIXED_ONE = BigInt(65536);

export interface RollResult {
    rolledInt: number;
    unit: string | null;
}

/** Decode a u64 Quality string into { fixed (integer), normalized (0..1 float) }. */
export function decodeQuality(raw: string): { fixed: number; normalized: number } {
    const bigRaw = BigInt(raw);
    if (bigRaw === 0n) return { fixed: 0, normalized: 0 };
    const fixed = Number(bigRaw / QUALITY_DIVISOR);
    const normalized = Math.min(1, fixed / 65536);
    return { fixed, normalized };
}

/** Encode a normalized float (0..1) back to a u64 Quality string. */
export function encodeQuality(normalized: number): string {
    const clamped = Math.max(0, Math.min(1, normalized));
    const fixed = BigInt(Math.round(clamped * 65536));
    return (fixed * QUALITY_DIVISOR).toString();
}

/**
 * Compute the rolled value for an enchantment given its quality and roll metadata.
 * Returns null for "special" roll kinds or when range metadata is missing.
 */
export function computeRolledValue(
    quality: string,
    rollKind: string | null,
    rollMin: number | null,
    rollMax: number | null,
    rollValue: number | null,
    rollUnit: string | null,
    rollIsNegative: boolean,
): RollResult | null {
    if (rollKind === "special" || rollKind == null) return null;

    if (rollKind === "fixed") {
        if (rollValue == null) return null;
        return { rolledInt: roundHalfUp(rollValue), unit: rollUnit };
    }

    if (rollKind === "range") {
        if (rollMin == null || rollMax == null) return null;
        const { normalized: q } = decodeQuality(quality);
        const t = rollIsNegative ? 1 - q : q;
        const rolled = rollMin + (rollMax - rollMin) * t;
        return { rolledInt: roundHalfUp(rolled), unit: rollUnit };
    }

    return null;
}

/** Replace {value} in rollText with the rolled integer (+ unit inline if part of text). */
export function formatRollText(rollText: string | null, result: RollResult | null): string | null {
    if (rollText == null) return null;
    if (!rollText.includes("{value}")) return rollText;
    if (result == null) return rollText;
    return rollText.replace("{value}", String(result.rolledInt));
}

/**
 * Compute the Quality string that produces the desired rolled integer.
 * Inverse of computeRolledValue for "range" enchantments.
 */
export function qualityFromDesiredValue(
    desired: number,
    rollMin: number,
    rollMax: number,
    rollIsNegative: boolean,
): string {
    if (rollMin === rollMax) return encodeQuality(0);
    const t = (desired - rollMin) / (rollMax - rollMin);
    const tClamped = Math.max(0, Math.min(1, t));
    const q = rollIsNegative ? 1 - tClamped : tClamped;
    return encodeQuality(q);
}

function roundHalfUp(x: number): number {
    return Math.floor(x + 0.5);
}

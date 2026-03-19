import { describe, expect, it } from "vitest";
import {
    computeRolledValue,
    decodeQuality,
    encodeQuality,
    formatRollText,
    qualityFromDesiredValue,
} from "./quality";

describe("decodeQuality", () => {
    it('decodes "0" to { fixed: 0, normalized: 0 }', () => {
        const result = decodeQuality("0");
        expect(result.fixed).toBe(0);
        expect(result.normalized).toBe(0);
    });

    it("decodes a known non-zero quality string", () => {
        // QUALITY_DIVISOR = 0xFFFF00010000 = 281470681808896
        // For normalized ≈ 0.68: fixed ≈ 44564, raw ≈ 44564 * 281470681808896
        const raw = BigInt(44564) * BigInt("281470681808896");
        const { fixed, normalized } = decodeQuality(raw.toString());
        expect(fixed).toBe(44564);
        expect(normalized).toBeCloseTo(44564 / 65536, 5);
    });

    it("clamps normalized to max 1.0", () => {
        // large value that would overflow past 1
        const raw = BigInt(70000) * BigInt("281470681808896");
        const { normalized } = decodeQuality(raw.toString());
        expect(normalized).toBe(1.0);
    });
});

describe("encodeQuality", () => {
    it("encodes 0 to '0'", () => {
        expect(encodeQuality(0)).toBe("0");
    });

    it("encodes 1.0 to max value string", () => {
        const result = encodeQuality(1.0);
        expect(typeof result).toBe("string");
        const { normalized } = decodeQuality(result);
        expect(normalized).toBeCloseTo(1.0, 3);
    });

    it("round-trips ~0.68", () => {
        const encoded = encodeQuality(0.68);
        const { normalized } = decodeQuality(encoded);
        expect(normalized).toBeCloseTo(0.68, 3);
    });
});

describe("computeRolledValue", () => {
    it("returns null for rollKind=special", () => {
        const result = computeRolledValue("0", "special", null, null, null, null, false);
        expect(result).toBeNull();
    });

    it("returns fixed value for rollKind=fixed", () => {
        const result = computeRolledValue("0", "fixed", null, null, 6, null, false);
        expect(result).not.toBeNull();
        expect(result!.rolledInt).toBe(6);
    });

    it("computes rolled value for range (q=0.68, min=3, max=10)", () => {
        const q = encodeQuality(0.68);
        const result = computeRolledValue(q, "range", 3, 10, null, "%", false);
        expect(result).not.toBeNull();
        // 3 + (10-3)*0.68 = 3 + 4.76 = 7.76 → rounds to 8
        expect(result!.rolledInt).toBe(8);
        expect(result!.unit).toBe("%");
    });

    it("uses t=1-q for negative range (q=0.6, min=10, max=15, isNeg=true)", () => {
        const q = encodeQuality(0.6);
        const result = computeRolledValue(q, "range", 10, 15, null, null, true);
        expect(result).not.toBeNull();
        // t = 1 - 0.6 = 0.4; 10 + (15-10)*0.4 = 10 + 2 = 12
        expect(result!.rolledInt).toBe(12);
    });

    it("returns min for q=0 (non-negative range)", () => {
        const q = encodeQuality(0);
        const result = computeRolledValue(q, "range", 3, 10, null, null, false);
        expect(result).not.toBeNull();
        expect(result!.rolledInt).toBe(3);
    });

    it("returns max for q=1 (non-negative range)", () => {
        const q = encodeQuality(1.0);
        const result = computeRolledValue(q, "range", 3, 10, null, null, false);
        expect(result).not.toBeNull();
        expect(result!.rolledInt).toBe(10);
    });

    it("returns min for min===max", () => {
        const q = encodeQuality(0.5);
        const result = computeRolledValue(q, "range", 5, 5, null, null, false);
        expect(result).not.toBeNull();
        expect(result!.rolledInt).toBe(5);
    });
});

describe("formatRollText", () => {
    it("replaces {value} in rollText with formatted rolled int", () => {
        const result = formatRollText("Damage increased by {value}%", { rolledInt: 7, unit: "%" });
        expect(result).toBe("Damage increased by 7%");
    });

    it("returns rollText unchanged when no {value} placeholder", () => {
        const result = formatRollText("Blunt", null);
        expect(result).toBe("Blunt");
    });

    it("returns null when rollText is null", () => {
        const result = formatRollText(null, { rolledInt: 7, unit: "%" });
        expect(result).toBeNull();
    });

    it("replaces {value} with just the number when unit is null", () => {
        const result = formatRollText("Plus {value} damage", { rolledInt: 5, unit: null });
        expect(result).toBe("Plus 5 damage");
    });
});

describe("qualityFromDesiredValue", () => {
    it("round-trips desired=7, min=3, max=10 (non-negative)", () => {
        const q = qualityFromDesiredValue(7, 3, 10, false);
        const result = computeRolledValue(q, "range", 3, 10, null, null, false);
        expect(result!.rolledInt).toBe(7);
    });

    it("round-trips desired=12, min=10, max=15 (negative)", () => {
        const q = qualityFromDesiredValue(12, 10, 15, true);
        const result = computeRolledValue(q, "range", 10, 15, null, null, true);
        expect(result!.rolledInt).toBe(12);
    });

    it("clamps at min when desired=min", () => {
        const q = qualityFromDesiredValue(3, 3, 10, false);
        const result = computeRolledValue(q, "range", 3, 10, null, null, false);
        expect(result!.rolledInt).toBe(3);
    });

    it("clamps at max when desired=max", () => {
        const q = qualityFromDesiredValue(10, 3, 10, false);
        const result = computeRolledValue(q, "range", 3, 10, null, null, false);
        expect(result!.rolledInt).toBe(10);
    });
});

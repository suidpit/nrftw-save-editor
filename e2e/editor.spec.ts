import { expect, test } from "@playwright/test";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const SAVE_FILE = path.resolve(__dirname, "../examples/char_save_1.cerimal");
const FILLED_SAVE_FILE = path.resolve(__dirname, "../examples/char_save_filled.cerimal");
const EXALTED_SAVE_FILE = path.resolve(__dirname, "../examples/char_exalted.cerimal");
const EDITABLE_EQUIPMENT_NAME = "Tattered Cerim Leggings";

async function loadSave(page: import("@playwright/test").Page, filePath: string) {
    await page.goto("/");
    await page.locator(".boot-status.ready").waitFor({ timeout: 15_000 });
    const fileInput = page.locator('input[type="file"][accept=".cerimal"]').first();
    await fileInput.setInputFiles(filePath);
    await page.locator(".main-layout").waitFor({ timeout: 10_000 });
}

async function inventoryItemSummary(
    page: import("@playwright/test").Page,
    index: number,
) {
    const item = page.locator(".inventory-item:not(.add-item)").nth(index);
    const label = await item.getAttribute("aria-label");
    await item.hover();
    const guid = (await page.locator(".item-tooltip .tooltip-guid").textContent())?.trim();
    expect(label).toBeTruthy();
    expect(guid).toBeTruthy();
    return {
        label: label!,
        guid: guid!,
    };
}

async function collectInventoryGuids(page: import("@playwright/test").Page) {
    const items = page.locator(".inventory-item:not(.add-item)");
    const guids: string[] = [];
    const count = await items.count();
    for (let index = 0; index < count; index += 1) {
        await items.nth(index).hover();
        const guid = (await page.locator(".item-tooltip .tooltip-guid").textContent())?.trim();
        expect(guid).toBeTruthy();
        guids.push(guid!);
    }
    return guids;
}

async function deleteInventoryItemByIndex(
    page: import("@playwright/test").Page,
    index: number,
) {
    await page.locator(".inventory-item:not(.add-item)").nth(index).click();
    page.once("dialog", (dialog) => dialog.accept());
    await page.getByRole("button", { name: "Delete", exact: true }).click();
}

async function deleteInventoryItemByGuid(
    page: import("@playwright/test").Page,
    guid: string,
) {
    const items = page.locator(".inventory-item:not(.add-item)");
    const count = await items.count();
    for (let index = 0; index < count; index += 1) {
        await items.nth(index).hover();
        const currentGuid = (await page.locator(".item-tooltip .tooltip-guid").textContent())?.trim();
        if (currentGuid === guid) {
            await deleteInventoryItemByIndex(page, index);
            return;
        }
    }
    throw new Error(`Inventory item with GUID ${guid} not found`);
}

async function createWildlingHelmet(page: import("@playwright/test").Page) {
    await page.locator(".inventory-item.add-item").click();
    await page.locator("#asset-type").selectOption("HelmDataAsset");
    await page.locator('button:has-text("Pick Asset…")').click();
    await page.locator('input[placeholder*="Search"]').fill("Wildling Helmet");
    await page.locator('.picker-row:has-text("Wildling Helmet")').click();
    await page.locator("#item-level").fill("6");
    await page.locator('.save-btn:has-text("Create")').click();
}

async function createMushroom(page: import("@playwright/test").Page) {
    await page.locator(".inventory-item.add-item").click();
    await page.locator("#asset-type").selectOption("FoodItemDataAsset");
    await page.locator('button:has-text("Pick Asset…")').click();
    await page.locator('input[placeholder*="Search"]').fill("Mushroom");
    await page.getByRole("button", { name: "Mushroom", exact: true }).click();
    await page.locator("#item-stack").fill("12");
    await page.locator('.save-btn:has-text("Create")').click();
}

async function createBattlecryRing(page: import("@playwright/test").Page) {
    await page.locator(".inventory-item.add-item").click();
    await page.locator("#asset-type").selectOption("RingsDataAsset");
    await page.locator('button:has-text("Pick Asset…")').click();
    await page.locator('input[placeholder*="Search"]').fill("Battlecry Ring");
    await page.locator('.picker-row:has-text("Battlecry Ring")').click();
    await page.locator('.save-btn:has-text("Create")').click();
}

async function createBandOfCalmness(page: import("@playwright/test").Page) {
    await page.locator(".inventory-item.add-item").click();
    await page.locator("#asset-type").selectOption("RingsDataAsset");
    await page.locator('button:has-text("Pick Asset…")').click();
    await page.locator('input[placeholder*="Search"]').fill("Band Of Calmness");
    await page.locator('.picker-row:has-text("Band Of Calmness")').click();
}

async function createFetidClub(page: import("@playwright/test").Page) {
    await page.locator(".inventory-item.add-item").click();
    await page.locator("#asset-type").selectOption("WeaponStaticDataAsset");
    await page.locator('button:has-text("Pick Asset…")').click();
    await page.locator('input[placeholder*="Search"]').fill("Fetid Club");
    await page.locator('.picker-row:has-text("Fetid Club")').click();
}

test.describe("Enchantment rolls (filled save)", () => {
    test.beforeEach(async ({ page }) => {
        await loadSave(page, FILLED_SAVE_FILE);
    });

    test("tooltip shows exalt stars for exalted items", async ({ page }) => {
        await loadSave(page, EXALTED_SAVE_FILE);
        const items = page.locator(".inventory-item:not(.add-item)");
        const count = await items.count();

        let foundExalt = false;
        for (let i = 0; i < count; i++) {
            await items.nth(i).hover();
            const stars = page.locator(".item-tooltip .tooltip-exalt-stars");
            if (await stars.count() > 0) {
                await expect(stars.first()).toContainText("★");
                foundExalt = true;
                break;
            }
        }
        expect(foundExalt, "expected at least one item with tooltip exalt stars").toBe(true);
    });

    test("tooltip shows computed roll text for enchanted items (not raw ranges)", async ({ page }) => {
        // char_save_filled.cerimal has enchanted items in inventory.
        // Verify at least one tooltip shows a specific rolled value rather than a raw "(min% - max%)" range string.
        const items = page.locator(".inventory-item:not(.add-item)");
        const count = await items.count();

        let foundEnchanted = false;
        for (let i = 0; i < count; i++) {
            await items.nth(i).hover();
            const tooltip = page.locator(".item-tooltip");
            const enchants = tooltip.locator(".tooltip-enchant");
            const enchantCount = await enchants.count();
            if (enchantCount === 0) continue;

            const tooltipText = await tooltip.textContent();
            // Raw range text like "(7% - 15%)" should never appear — always resolved to a single value
            expect(tooltipText).not.toMatch(/\(\d+%\s*-\s*\d+%\)/);
            foundEnchanted = true;
            break;
        }
        expect(foundEnchanted).toBe(true);
    });
});

test.describe("Editor Tab", () => {
    test.beforeEach(async ({ page }) => {
        await loadSave(page, SAVE_FILE);
    });

    test("equipment panel is removed", async ({ page }) => {
        await expect(page.locator(".equipment-panel")).toHaveCount(0);
    });

    test("editing Level updates edit stash", async ({ page }) => {
        const levelInput = page.locator('input[data-field="Level"]');
        await levelInput.fill("20");
        await levelInput.blur();
        await expect(page.locator(".download-btn")).toBeVisible();
    });

    test("hovering inventory item shows tooltip with content", async ({ page }) => {
        const firstItem = page.locator(".inventory-item:not(.add-item)").first();
        await firstItem.waitFor({ timeout: 10_000 });
        await firstItem.hover();
        const tooltip = page.locator(".item-tooltip");
        await expect(tooltip).toBeVisible();
        await expect(tooltip.locator(".tooltip-name")).toBeVisible();
        await expect(tooltip.locator(".tooltip-rarity")).toBeVisible();
    });

    test("inventory tooltip stays within the viewport near the right edge", async ({ page }) => {
        const items = page.locator(".inventory-item:not(.add-item)");
        await expect(items.first()).toBeVisible({ timeout: 10_000 });

        const rightmostIndex = await items.evaluateAll((elements) => {
            let maxRight = -Infinity;
            let index = 0;
            elements.forEach((element, currentIndex) => {
                const right = element.getBoundingClientRect().right;
                if (right > maxRight) {
                    maxRight = right;
                    index = currentIndex;
                }
            });
            return index;
        });

        await items.nth(rightmostIndex).hover();

        const tooltip = page.locator(".item-tooltip");
        await expect(tooltip).toBeVisible();

        const tooltipBox = await tooltip.boundingBox();
        expect(tooltipBox).not.toBeNull();

        const viewport = page.viewportSize();
        expect(viewport).not.toBeNull();

        expect(tooltipBox!.x).toBeGreaterThanOrEqual(0);
        expect(tooltipBox!.y).toBeGreaterThanOrEqual(0);
        expect(tooltipBox!.x + tooltipBox!.width).toBeLessThanOrEqual(viewport!.width);
        expect(tooltipBox!.y + tooltipBox!.height).toBeLessThanOrEqual(viewport!.height);
    });

    test("create flow stages a new inventory item", async ({ page }) => {
        const items = page.locator(".inventory-item:not(.add-item)");
        const beforeCount = await items.count();

        await page.locator(".inventory-item.add-item").click();
        await page.locator("#asset-type").selectOption("HelmDataAsset");
        await page.locator('button:has-text("Pick Asset…")').click();
        await page.locator('input[placeholder*="Search"]').fill("Wildling Helmet");
        await page.locator('.picker-row:has-text("Wildling Helmet")').click();
        await page.locator("#item-level").fill("6");
        await expect(page.locator("#item-durability")).toHaveValue("100");
        await expect(page.locator("#item-durability")).toBeDisabled();
        await page.locator('button:has-text("+ Add Enchantment")').click();
        await page.locator('input[placeholder="Search enchantments…"]').fill("Regen Focus In Combat");
        const enchantRow = page.locator('.picker-row:has-text("Regen Focus In Combat")');
        await expect(enchantRow).toContainText("Gain 0.38 Focus Regeneration");
        await enchantRow.click();
        await page.locator('.save-btn:has-text("Create")').click();

        await expect(page.locator(".download-btn")).toBeVisible();
        await expect(page.locator(".inventory-item:not(.add-item)")).toHaveCount(beforeCount + 1);
        await expect(page.locator(".inventory-item:not(.add-item)").last()).toHaveAttribute("aria-label", "Wildling Helmet");
    });

    test("create flow allows stack for stackable assets", async ({ page }) => {
        const items = page.locator(".inventory-item:not(.add-item)");
        const beforeCount = await items.count();

        await page.locator(".inventory-item.add-item").click();
        await page.locator("#asset-type").selectOption("FoodItemDataAsset");
        await page.locator('button:has-text("Pick Asset…")').click();
        await page.locator('input[placeholder*="Search"]').fill("Mushroom");
        await page.getByRole("button", { name: "Mushroom", exact: true }).click();
        await expect(page.locator("#item-stack")).toHaveValue("1");
        await page.locator("#item-stack").fill("12");
        await page.locator('.save-btn:has-text("Create")').click();

        const createdItem = page.locator('.inventory-item:not(.add-item)[aria-label="Mushroom"]').last();
        await expect(items).toHaveCount(beforeCount + 1);
        await expect(createdItem.locator(".item-stack")).toHaveText("12");

        await createdItem.hover();
        await expect(page.locator(".item-tooltip")).toContainText("x12");
    });

    test("added enchantments are removed from the picker", async ({ page }) => {
        await page.locator(".inventory-item.add-item").click();
        await page.locator("#asset-type").selectOption("HelmDataAsset");
        await page.locator('button:has-text("Pick Asset…")').click();
        await page.locator('input[placeholder*="Search"]').fill("Wildling Helmet");
        await page.locator('.picker-row:has-text("Wildling Helmet")').click();

        await page.locator('button:has-text("+ Add Enchantment")').click();
        await page.locator('input[placeholder="Search enchantments…"]').fill("Regen Focus In Combat");
        await page.locator('.picker-row:has-text("Regen Focus In Combat")').click();

        await page.locator('button:has-text("+ Add Enchantment")').click();
        await page.locator('input[placeholder="Search enchantments…"]').fill("Regen Focus In Combat");
        await expect(
            page.locator('.picker-row:has-text("Regen Focus In Combat")'),
        ).toHaveCount(0);
        await expect(page.locator(".picker-empty")).toContainText("No results.");
    });

    test("enchantment picker respects compatibility and item restrictions", async ({ page }) => {
        await page.locator(".inventory-item.add-item").click();
        await page.locator("#asset-type").selectOption("HelmDataAsset");
        await page.locator('button:has-text("Pick Asset…")').click();
        await page.locator('input[placeholder*="Search"]').fill("Wildling Helmet");
        await page.locator('.picker-row:has-text("Wildling Helmet")').click();

        await page.locator('button:has-text("+ Add Enchantment")').click();
        await page.locator('input[placeholder="Search enchantments…"]').fill("Spend Health Instead Focus");
        await expect(page.locator(".picker-empty")).toContainText("No results.");
    });

    test("create flow preloads unique item enchantments", async ({ page }) => {
        const items = page.locator(".inventory-item:not(.add-item)");
        const beforeCount = await items.count();

        await page.locator(".inventory-item.add-item").click();
        await page.locator("#asset-type").selectOption("RingsDataAsset");
        await page.locator('button:has-text("Pick Asset…")').click();
        await page.locator('input[placeholder*="Search"]').fill("Battlecry Ring");
        await expect(page.locator('.picker-row:has-text("Battlecry Ring") .picker-tag')).toHaveText("Catalog Defaults");
        await page.locator('.picker-row:has-text("Battlecry Ring")').click();

        await expect(
            page.getByText(
                "Ring rarity and enchantments are fixed.",
            ),
        ).toBeVisible();
        await expect(page.locator("#item-quality")).toHaveValue("3");
        await expect(page.locator("#item-quality")).toBeDisabled();
        await expect(page.locator("#item-durability")).toHaveValue("100");
        await expect(page.locator("#item-durability")).toBeDisabled();
        await expect(page.locator(".enchant-row")).toHaveCount(1);
        await expect(page.locator(".enchant-row")).toContainText([
            "Spend Health Instead Focus",
        ]);
        await expect(page.locator(".enchant-row")).toContainText("Spend Health instead of Focus");
        await expect(page.locator('button:has-text("+ Add Enchantment")')).toBeDisabled();
        await expect(page.locator('button[aria-label="Move enchantment up"]')).toHaveCount(0);
        await expect(page.locator('button[aria-label="Remove enchantment"]')).toHaveCount(0);
        await expect(page.locator('.catalog-link')).toHaveAttribute("href", /norestforthewicked\.gg\/db\/trinkets\/ring\//);

        await page.locator('.save-btn:has-text("Create")').click();

        await expect(page.locator(".download-btn")).toBeVisible();
        await expect(items).toHaveCount(beforeCount + 1);
        await expect(items.last()).toHaveAttribute("aria-label", "Battlecry Ring");
    });

    test("editing a unique item keeps quality and enchantments locked", async ({ page }) => {
        await page.locator(".inventory-item.add-item").click();
        await page.locator("#asset-type").selectOption("RingsDataAsset");
        await page.locator('button:has-text("Pick Asset…")').click();
        await page.locator('input[placeholder*="Search"]').fill("Battlecry Ring");
        await page.locator('.picker-row:has-text("Battlecry Ring")').click();
        await page.locator('.save-btn:has-text("Create")').click();

        const createdItem = page.locator('.inventory-item:not(.add-item)[aria-label="Battlecry Ring"]');
        await createdItem.click();

        await expect(page.locator("#item-quality")).toHaveValue("3");
        await expect(page.locator("#item-quality")).toBeDisabled();
        await expect(page.locator(".enchant-row")).toHaveCount(1);
        await expect(page.locator('button:has-text("+ Add Enchantment")')).toBeDisabled();
        await expect(page.locator('button[aria-label="Move enchantment up"]')).toHaveCount(0);
        await expect(page.locator('button[aria-label="Remove enchantment"]')).toHaveCount(0);
    });

    test("rings with preset enchantments stay fixed even when not unique", async ({ page }) => {
        await createBandOfCalmness(page);

        await expect(page.locator("#item-quality")).toHaveValue("1");
        await expect(page.locator("#item-quality")).toBeDisabled();
        await expect(page.locator("#item-quality option[value=\"3\"]")).toHaveCount(0);
        await expect(page.locator(".enchant-row")).toHaveCount(2);
        await expect(page.locator(".enchant-row")).toContainText([
            "Max Focus Increased",
            "Focus Gain Increased",
        ]);
        await expect(
            page.getByText("Ring rarity and enchantments are fixed."),
        ).toBeVisible();
        await expect(page.locator('button:has-text("+ Add Enchantment")')).toBeDisabled();
        await expect(page.locator('button[aria-label="Move enchantment up"]')).toHaveCount(0);
        await expect(page.locator('button[aria-label="Remove enchantment"]')).toHaveCount(0);
    });

    test("create flow preloads catalog runes for gold weapons", async ({ page }) => {
        await createFetidClub(page);

        await expect(page.locator("#item-quality")).toHaveValue("3");
        await expect(page.locator(".field-section").filter({ hasText: "Runes" })).toBeVisible();
        await expect(page.locator(".static-row")).toHaveCount(4);
        await expect(page.locator(".static-row")).toContainText([
            "Crushing Quad",
            "Pole Flurry",
            "Whirlwind",
            "Berserk Strike",
        ]);
        await expect(
            page.locator(".field-section").filter({ hasText: "Enchantments" }).locator(".enchant-row"),
        ).toHaveCount(2);
        await expect(page.locator(".catalog-link")).toHaveAttribute("href", /norestforthewicked\.gg\/db\/weapons\/great_club\//);
    });

    test("non-unique items cannot be set to gold quality", async ({ page }) => {
        await page.locator(".inventory-item.add-item").click();
        await page.locator("#asset-type").selectOption("HelmDataAsset");
        await page.locator('button:has-text("Pick Asset…")').click();
        await page.locator('input[placeholder*="Search"]').fill("Wildling Helmet");
        await page.locator('.picker-row:has-text("Wildling Helmet")').click();

        await expect(page.locator("#item-quality")).toHaveValue("0");
        await expect(page.locator("#item-quality option[value=\"3\"]")).toHaveCount(0);
    });

    test("edited item survives export and reload", async ({ page }) => {
        const targetItem = page.locator(
            `.inventory-item:not(.add-item)[aria-label="${EDITABLE_EQUIPMENT_NAME}"]`,
        );
        await targetItem.hover();
        const originalTooltip = page.locator(".item-tooltip");
        const originalGuid = (await originalTooltip.locator(".tooltip-guid").textContent())?.trim();
        expect(originalGuid).toBeTruthy();

        await targetItem.click();

        await page.locator("#item-level").fill("9");
        await page.locator("#item-durability").fill("150");
        await page.locator('.save-btn:has-text("Save")').click();

        await targetItem.hover();
        await expect(page.locator(".item-tooltip")).toContainText("Lv 9");
        await expect(page.locator(".item-tooltip")).toContainText("Dur: 150");

        const downloadPromise = page.waitForEvent("download");
        await page.locator(".download-btn").click();
        const download = await downloadPromise;
        const downloadedPath = await download.path();
        expect(downloadedPath).not.toBeNull();

        await loadSave(page, downloadedPath!);

        const reloadedItem = page.locator(
            `.inventory-item:not(.add-item)[aria-label="${EDITABLE_EQUIPMENT_NAME}"]`,
        );
        await reloadedItem.hover();
        await expect(page.locator(".item-tooltip")).toContainText(originalGuid!);
        await expect(page.locator(".item-tooltip")).toContainText("Lv 9");
        await expect(page.locator(".item-tooltip")).toContainText("Dur: 150");
    });

    test("created item is not duplicated by repeated downloads", async ({ page }) => {
        const items = page.locator(".inventory-item:not(.add-item)");
        const beforeCount = await items.count();

        await page.locator(".inventory-item.add-item").click();
        await page.locator("#asset-type").selectOption("HelmDataAsset");
        await page.locator('button:has-text("Pick Asset…")').click();
        await page.locator('input[placeholder*="Search"]').fill("Wildling Helmet");
        await page.locator('.picker-row:has-text("Wildling Helmet")').click();
        await page.locator('.save-btn:has-text("Create")').click();

        await expect(items).toHaveCount(beforeCount + 1);

        const firstDownloadPromise = page.waitForEvent("download");
        await page.locator(".download-btn").click();
        const firstDownload = await firstDownloadPromise;
        const firstDownloadedPath = await firstDownload.path();
        expect(firstDownloadedPath).not.toBeNull();

        await expect(page.locator(".download-btn")).toHaveCount(0);

        await loadSave(page, firstDownloadedPath!);
        await expect(page.locator(".inventory-item:not(.add-item)")).toHaveCount(beforeCount + 1);
        await expect(page.locator(".download-btn")).toHaveCount(0);

        await page.locator(".inventory-item:not(.add-item)").first().click();
        await page.locator("#item-level").fill("10");
        await page.locator("#item-level").blur();
        await page.locator('.save-btn:has-text("Save")').click();
        await expect(page.locator(".download-btn")).toBeVisible();

        const secondDownloadPromise = page.waitForEvent("download");
        await page.locator(".download-btn").click();
        const secondDownload = await secondDownloadPromise;
        const secondDownloadedPath = await secondDownload.path();
        expect(secondDownloadedPath).not.toBeNull();

        await loadSave(page, secondDownloadedPath!);
        await expect(page.locator(".inventory-item:not(.add-item)")).toHaveCount(beforeCount + 1);
    });

    test("new range enchantment defaults to max roll", async ({ page }) => {
        // "Equip Load Decreased": unique name, range 20..30, helm-compatible
        // roll_text: "Equip Load reduced by {value}%"
        // When added as new, quality should default to max → rolled = 30
        await page.locator(".inventory-item.add-item").click();
        await page.locator("#asset-type").selectOption("HelmDataAsset");
        await page.locator('button:has-text("Pick Asset…")').click();
        await page.locator('input[placeholder*="Search"]').fill("Wildling Helmet");
        await page.locator('.picker-row:has-text("Wildling Helmet")').click();

        await page.locator('button:has-text("+ Add Enchantment")').click();
        await page.locator('input[placeholder="Search enchantments…"]').fill("Equip Load Decreased");
        await page.locator('.picker-row:has-text("Equip Load Decreased")').click();

        // Slider and number input should appear for range enchantments
        const slider = page.locator(".roll-slider");
        const numberInput = page.locator(".roll-number");
        await expect(slider).toBeVisible();
        await expect(numberInput).toBeVisible();

        // Default should be max (30)
        await expect(numberInput).toHaveValue("30");
        await expect(slider).toHaveValue("30");

        // Effect text should show max roll value
        await expect(page.locator(".modifier-effect")).toContainText("Equip Load reduced by 30%");
    });

    test("roll number input updates enchantment display text", async ({ page }) => {
        // "Equip Load Decreased": range 20..30
        await page.locator(".inventory-item.add-item").click();
        await page.locator("#asset-type").selectOption("HelmDataAsset");
        await page.locator('button:has-text("Pick Asset…")').click();
        await page.locator('input[placeholder*="Search"]').fill("Wildling Helmet");
        await page.locator('.picker-row:has-text("Wildling Helmet")').click();

        await page.locator('button:has-text("+ Add Enchantment")').click();
        await page.locator('input[placeholder="Search enchantments…"]').fill("Equip Load Decreased");
        await page.locator('.picker-row:has-text("Equip Load Decreased")').click();

        // Change number input to min (20)
        const numberInput = page.locator(".roll-number");
        await numberInput.fill("20");
        await numberInput.dispatchEvent("change");

        await expect(numberInput).toHaveValue("20");
        await expect(page.locator(".modifier-effect")).toContainText("Equip Load reduced by 20%");
    });

    test("enchantment roll quality survives download and reload", async ({ page }) => {
        // "Equip Load Decreased": range 20..30, unique, helm-compatible
        await page.locator(".inventory-item.add-item").click();
        await page.locator("#asset-type").selectOption("HelmDataAsset");
        await page.locator('button:has-text("Pick Asset…")').click();
        await page.locator('input[placeholder*="Search"]').fill("Wildling Helmet");
        await page.locator('.picker-row:has-text("Wildling Helmet")').click();

        await page.locator('button:has-text("+ Add Enchantment")').click();
        await page.locator('input[placeholder="Search enchantments…"]').fill("Equip Load Decreased");
        await page.locator('.picker-row:has-text("Equip Load Decreased")').click();

        // Set roll to 25 (between min=20 and max=30)
        const numberInput = page.locator(".roll-number");
        await numberInput.fill("25");
        await numberInput.dispatchEvent("change");
        await expect(page.locator(".modifier-effect")).toContainText("Equip Load reduced by 25%");

        await page.locator('.save-btn:has-text("Create")').click();

        // Verify tooltip shows the resolved roll (not raw range)
        const newItem = page.locator('.inventory-item:not(.add-item)[aria-label="Wildling Helmet"]').last();
        await newItem.hover();
        const tooltip = page.locator(".item-tooltip");
        await expect(tooltip.locator(".tooltip-enchant")).toContainText("Equip Load reduced by 25%");
        await expect(tooltip.locator(".tooltip-enchant")).not.toContainText("(20%");

        // Download, reload, and verify the roll value was preserved
        const downloadPromise = page.waitForEvent("download");
        await page.locator(".download-btn").click();
        const download = await downloadPromise;
        const downloadedPath = await download.path();
        expect(downloadedPath).not.toBeNull();

        await loadSave(page, downloadedPath!);

        const reloadedItem = page.locator('.inventory-item:not(.add-item)[aria-label="Wildling Helmet"]').last();
        await reloadedItem.hover();
        await expect(page.locator(".item-tooltip .tooltip-enchant")).toContainText("Equip Load reduced by 25%");
    });

    test("mixed create edit and delete flow survives download and reload", async ({ page }) => {
        const items = page.locator(".inventory-item:not(.add-item)");
        const beforeCount = await items.count();

        const firstOriginal = await inventoryItemSummary(page, 0);
        const middleOriginal = await inventoryItemSummary(page, 1);
        const lastOriginal = await inventoryItemSummary(page, beforeCount - 1);

        await page
            .locator(`.inventory-item:not(.add-item)[aria-label="${EDITABLE_EQUIPMENT_NAME}"]`)
            .click();
        await page.locator("#item-level").fill("11");
        await page.locator("#item-durability").fill("145");
        await page.locator('.save-btn:has-text("Save")').click();

        await createWildlingHelmet(page);
        await expect(items).toHaveCount(beforeCount + 1);
        const wildlingGuid = (await inventoryItemSummary(page, beforeCount)).guid;

        await createMushroom(page);
        await expect(items).toHaveCount(beforeCount + 2);
        const mushroomGuid = (await inventoryItemSummary(page, beforeCount + 1)).guid;
        await items.nth(beforeCount + 1).hover();
        await expect(page.locator(".item-tooltip")).toContainText("x12");

        await deleteInventoryItemByIndex(page, beforeCount - 1);
        await expect(items).toHaveCount(beforeCount + 1);

        await deleteInventoryItemByIndex(page, 0);
        await expect(items).toHaveCount(beforeCount);

        await deleteInventoryItemByGuid(page, middleOriginal.guid);
        await expect(items).toHaveCount(beforeCount - 1);

        await createBattlecryRing(page);
        await expect(items).toHaveCount(beforeCount);
        const battlecryGuid = (await inventoryItemSummary(page, beforeCount - 1)).guid;

        const downloadPromise = page.waitForEvent("download");
        await page.locator(".download-btn").click();
        const download = await downloadPromise;
        const downloadedPath = await download.path();
        expect(downloadedPath).not.toBeNull();

        await loadSave(page, downloadedPath!);
        await expect(items).toHaveCount(beforeCount);

        const reloadedGuids = await collectInventoryGuids(page);
        expect(reloadedGuids).not.toContain(firstOriginal.guid);
        expect(reloadedGuids).not.toContain(middleOriginal.guid);
        expect(reloadedGuids).not.toContain(lastOriginal.guid);
        expect(reloadedGuids).toContain(wildlingGuid);
        expect(reloadedGuids).toContain(mushroomGuid);
        expect(reloadedGuids).toContain(battlecryGuid);

        await items.nth(beforeCount - 2).hover();
        await expect(page.locator(".item-tooltip .tooltip-guid")).toHaveText(mushroomGuid);
        await expect(page.locator(".item-tooltip")).toContainText("x12");

        await items.nth(beforeCount - 1).hover();
        await expect(page.locator(".item-tooltip .tooltip-guid")).toHaveText(battlecryGuid);
    });

    test("exalt stepper hidden when item is not fully slotted", async ({ page }) => {
        await page.locator(".inventory-item.add-item").click();
        await page.locator("#asset-type").selectOption("HelmDataAsset");
        await page.locator('button:has-text("Pick Asset…")').click();
        await page.locator('input[placeholder*="Search"]').fill("Wildling Helmet");
        await page.locator('.picker-row:has-text("Wildling Helmet")').click();

        // Add 1 enchantment (blue rarity cap = 3, so not fully slotted)
        await page.locator('button:has-text("+ Add Enchantment")').click();
        await page.locator('input[placeholder="Search enchantments…"]').fill("Regen Focus In Combat");
        await page.locator('.picker-row:has-text("Regen Focus In Combat")').click();

        // 1/3 slots filled → no exalt stepper, hint visible
        await expect(page.locator(".exalt-control")).toHaveCount(0);
        await expect(page.getByText(/Fill all 3 enchantment slots/)).toBeVisible();
    });

    test("exalt stepper appears when all enchantment slots are filled", async ({ page }) => {
        await page.locator(".inventory-item.add-item").click();
        await page.locator("#asset-type").selectOption("HelmDataAsset");
        await page.locator('button:has-text("Pick Asset…")').click();
        await page.locator('input[placeholder*="Search"]').fill("Wildling Helmet");
        await page.locator('.picker-row:has-text("Wildling Helmet")').click();

        // Fill all 3 blue-rarity slots
        await page.locator('button:has-text("+ Add Enchantment")').click();
        await page.locator('input[placeholder="Search enchantments…"]').fill("Regen Focus In Combat");
        await page.locator('.picker-row:has-text("Regen Focus In Combat")').click();

        await page.locator('button:has-text("+ Add Enchantment")').click();
        await page.locator('input[placeholder="Search enchantments…"]').fill("Equip Load Decreased");
        await page.locator('.picker-row:has-text("Equip Load Decreased")').click();

        await page.locator('button:has-text("+ Add Enchantment")').click();
        await page.locator('input[placeholder="Search enchantments…"]').fill("");
        await page.locator('.picker-row').first().click();

        // 3/3 slots filled → exalt stepper visible per row + total counter
        await expect(page.locator(".exalt-control")).toHaveCount(3);
        await expect(page.locator(".exalt-total")).toBeVisible();
        await expect(page.locator(".exalt-total")).toContainText("Exalt: 0/4");
    });

    test("exalt total caps at 4 across all enchantments", async ({ page }) => {
        await page.locator(".inventory-item.add-item").click();
        await page.locator("#asset-type").selectOption("HelmDataAsset");
        await page.locator('button:has-text("Pick Asset…")').click();
        await page.locator('input[placeholder*="Search"]').fill("Wildling Helmet");
        await page.locator('.picker-row:has-text("Wildling Helmet")').click();

        // Fill all 3 blue-rarity slots
        await page.locator('button:has-text("+ Add Enchantment")').click();
        await page.locator('input[placeholder="Search enchantments…"]').fill("Regen Focus In Combat");
        await page.locator('.picker-row:has-text("Regen Focus In Combat")').click();

        await page.locator('button:has-text("+ Add Enchantment")').click();
        await page.locator('input[placeholder="Search enchantments…"]').fill("Equip Load Decreased");
        await page.locator('.picker-row:has-text("Equip Load Decreased")').click();

        await page.locator('button:has-text("+ Add Enchantment")').click();
        await page.locator('input[placeholder="Search enchantments…"]').fill("");
        await page.locator('.picker-row').first().click();

        // Click + 4 times on the first enchantment to reach the per-item cap
        const firstIncreaseBtn = page.locator('.exalt-control button[aria-label="Increase exalt"]').first();
        await firstIncreaseBtn.click();
        await firstIncreaseBtn.click();
        await firstIncreaseBtn.click();
        await firstIncreaseBtn.click();

        // Total = 4, EXALTED label visible, all + buttons disabled
        await expect(page.locator(".exalt-total")).toContainText("Exalt: 4/4");
        await expect(page.locator(".exalt-label")).toContainText("EXALTED");
        const increaseButtons = page.locator('.exalt-control button[aria-label="Increase exalt"]');
        const btnCount = await increaseButtons.count();
        for (let i = 0; i < btnCount; i++) {
            await expect(increaseButtons.nth(i)).toBeDisabled();
        }
    });
});

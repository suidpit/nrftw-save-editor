import { expect, test } from "@playwright/test";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const DEST_SAVE = path.resolve(__dirname, "../examples/char_save_1.cerimal");
const SOURCE_SAVE = path.resolve(__dirname, "../examples/char_save_filled.cerimal");

async function loadSave(page: import("@playwright/test").Page, filePath: string) {
    await page.goto("/");
    await page.locator(".boot-status.ready").waitFor({ timeout: 15_000 });
    const fileInput = page.locator('input[type="file"][accept=".cerimal"]').first();
    await fileInput.setInputFiles(filePath);
    await page.locator(".main-layout").waitFor({ timeout: 10_000 });
}

test("import appearance panel is visible", async ({ page }) => {
    await loadSave(page, DEST_SAVE);
    await expect(page.locator(".appearance-panel")).toBeVisible();
    await expect(page.locator(".appearance-panel .import-btn")).toBeVisible();
});

test("import appearance from another save stages customization edits", async ({ page }) => {
    await loadSave(page, DEST_SAVE);

    // No pending changes yet — download button hidden
    await expect(page.locator(".download-btn")).not.toBeVisible();

    // Import appearance from source save via the hidden file input
    await page.locator(".appearance-panel input[type='file']").setInputFiles(SOURCE_SAVE);

    // Status message should appear
    const status = page.locator(".appearance-status");
    await status.waitFor({ timeout: 5_000 });
    await expect(status).toContainText("Imported");
    await expect(status).toContainText("char_save_filled.cerimal");

    // Pending customization edits staged → download button now visible
    await expect(page.locator(".download-btn")).toBeVisible();
});

test("import appearance is idempotent — re-importing same source overwrites", async ({ page }) => {
    await loadSave(page, DEST_SAVE);

    const input = page.locator(".appearance-panel input[type='file']");

    await input.setInputFiles(SOURCE_SAVE);
    await page.locator(".appearance-status").waitFor({ timeout: 5_000 });

    // Import same source again — should succeed without error
    await input.setInputFiles(SOURCE_SAVE);
    await expect(page.locator(".appearance-status")).toContainText("Imported");
    await expect(page.locator(".download-btn")).toBeVisible();
});

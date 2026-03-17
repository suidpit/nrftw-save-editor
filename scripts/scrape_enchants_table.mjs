#!/usr/bin/env node

import fs from "node:fs/promises";
import path from "node:path";
import { chromium } from "@playwright/test";

const DEFAULT_URL = "https://www.norestforthewicked.gg/db/enchants";

function parseArgs(argv) {
    const options = {
        url: DEFAULT_URL,
        output: null,
        headless: true,
    };

    for (let i = 0; i < argv.length; i += 1) {
        const arg = argv[i];
        if (arg === "--output") {
            options.output = argv[i + 1] ?? null;
            i += 1;
            continue;
        }
        if (arg === "--headed") {
            options.headless = false;
            continue;
        }
        if (arg === "--url") {
            options.url = argv[i + 1] ?? DEFAULT_URL;
            i += 1;
        }
    }

    return options;
}

async function extractHeaders(page) {
    return page.locator("table thead th").evaluateAll((nodes) =>
        nodes.map((node) => (node.textContent || "").replace(/\s+/g, " ").trim()),
    );
}

async function extractRows(page, headers) {
    return page.locator("table tbody tr").evaluateAll((nodes, headerRow) =>
        nodes.map((row) => {
            const cells = Array.from(row.querySelectorAll("td,th"));
            const values = cells.map((cell) => (cell.textContent || "").replace(/\s+/g, " ").trim());
            const record = {};
            headerRow.forEach((header, index) => {
                record[header || `column_${index}`] = values[index] ?? "";
            });
            return record;
        }),
        headers,
    );
}

async function currentPageNumber(page) {
    const selected = page.locator(".p-paginator-page.p-paginator-page-selected");
    const text = (await selected.textContent()) ?? "1";
    return Number.parseInt(text.trim(), 10);
}

async function hasNextPage(page) {
    return page.locator('button[aria-label="Next Page"]:not(.p-disabled)').count();
}

async function goToNextPage(page) {
    const before = await currentPageNumber(page);
    await page.locator('button[aria-label="Next Page"]').click();
    await page.waitForFunction(
        (pageNumber) => {
            const node = document.querySelector(".p-paginator-page.p-paginator-page-selected");
            return node && Number.parseInt((node.textContent || "0").trim(), 10) === pageNumber + 1;
        },
        before,
    );
    await page.waitForLoadState("networkidle");
}

async function scrapeAllPages(page) {
    const headers = await extractHeaders(page);
    const pages = [];

    while (true) {
        const pageNumber = await currentPageNumber(page);
        const rows = await extractRows(page, headers);
        pages.push({
            page: pageNumber,
            rowCount: rows.length,
            rows,
        });

        if (!(await hasNextPage(page))) {
            break;
        }
        await goToNextPage(page);
    }

    return {
        url: page.url(),
        headers,
        pages,
        rows: pages.flatMap((entry) => entry.rows),
    };
}

async function main() {
    const options = parseArgs(process.argv.slice(2));
    const browser = await chromium.launch({ headless: options.headless });
    const page = await browser.newPage();

    try {
        await page.goto(options.url, { waitUntil: "domcontentloaded" });
        await page.waitForLoadState("networkidle");
        await page.locator("table tbody tr").first().waitFor({ timeout: 30_000 });

        const scraped = await scrapeAllPages(page);
        const summary = {
            pageCount: scraped.pages.length,
            totalRows: scraped.rows.length,
            firstRows: scraped.rows.slice(0, 5),
            lastRows: scraped.rows.slice(-5),
        };

        if (options.output) {
            const outputPath = path.resolve(options.output);
            await fs.mkdir(path.dirname(outputPath), { recursive: true });
            await fs.writeFile(outputPath, JSON.stringify(scraped, null, 2) + "\n", "utf-8");
            console.log(`wrote ${outputPath}`);
        }

        console.log(JSON.stringify(summary, null, 2));
    } finally {
        await page.close();
        await browser.close();
    }
}

main().catch((error) => {
    console.error(error);
    process.exitCode = 1;
});

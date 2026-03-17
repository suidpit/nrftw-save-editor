import { test, expect } from '@playwright/test'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const SAVE_FILE = path.resolve(__dirname, '../examples/char_save_filled.cerimal')

test.describe('Landing Page', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/')
    await page.locator('.boot-status.ready').waitFor({ timeout: 15_000 })
  })

  test('shows landing page with drop zone', async ({ page }) => {
    await expect(page.locator('.landing-page')).toBeVisible()
    await expect(page.locator('.drop-zone')).toBeVisible()
  })

  test('drop zone has drag-and-drop text', async ({ page }) => {
    await expect(page.locator('.drop-zone')).toContainText("Drag")
  })

  test('drop zone has file upload input', async ({ page }) => {
    await expect(page.locator('.drop-zone input[type="file"]')).toBeAttached()
  })

  test('uploading file transitions to main layout', async ({ page }) => {
    const fileInput = page.locator('input[type="file"][accept=".cerimal"]').first()
    await fileInput.setInputFiles(SAVE_FILE)
    await page.locator('.main-layout').waitFor({ timeout: 10_000 })
    await expect(page.locator('.landing-page')).not.toBeVisible()
  })

  test('main layout shows Editor, Inspector, Catalog tabs', async ({ page }) => {
    const fileInput = page.locator('input[type="file"][accept=".cerimal"]').first()
    await fileInput.setInputFiles(SAVE_FILE)
    await page.locator('.main-layout').waitFor({ timeout: 10_000 })
    await expect(page.locator('button.tab', { hasText: 'Editor' })).toBeVisible()
    await expect(page.locator('button.tab', { hasText: 'Inspector' })).toBeVisible()
    await expect(page.locator('button.tab', { hasText: 'Catalog' })).toBeVisible()
  })

  test('default active tab is Editor', async ({ page }) => {
    const fileInput = page.locator('input[type="file"][accept=".cerimal"]').first()
    await fileInput.setInputFiles(SAVE_FILE)
    await page.locator('.main-layout').waitFor({ timeout: 10_000 })
    await expect(page.locator('button.tab.active')).toHaveText('Editor')
  })
})

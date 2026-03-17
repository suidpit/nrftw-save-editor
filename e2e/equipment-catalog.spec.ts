import { test, expect } from '@playwright/test'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const SAVE_FILE = path.resolve(__dirname, '../examples/char_save_filled.cerimal')

// Equipment slots from the save file → expected catalog display names
const EQUIPMENT = [
  { slot: 'RightHand', guid: '1023863541289577995', name: 'Laquered Bow' },
  { slot: 'LeftHand', guid: '716007893309524987', name: 'Wooden Shield' },
  { slot: 'Helmet', guid: '1838235934543696561', name: 'Wildling Helmet' },
] as const

test.describe('Equipment GUID → Catalog lookup', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/')

    // Wait for WASM boot
    await page.locator('.boot-status.ready').waitFor({ timeout: 15_000 })

    // Upload save file via the hidden file input
    const fileInput = page.locator('input[type="file"][accept=".cerimal"]').first()
    await fileInput.setInputFiles(SAVE_FILE)

    // Wait for file to be parsed and main layout to appear
    await page.locator('.main-layout').waitFor({ timeout: 10_000 })

    // Switch to the Inspector tab where the tree lives
    await page.locator('button.tab', { hasText: 'Inspector' }).click()
    await page.locator('.tree .node').first().waitFor({ timeout: 8_000 })
  })

  for (const equip of EQUIPMENT) {
    test(`${equip.slot} → ${equip.name}`, async ({ page }) => {
      // Expand the equipment slot node to reveal its ASSETGUID child
      const slotNode = page.locator('.tree .node', {
        has: page.locator('.key', { hasText: new RegExp(`^${equip.slot}$`) }),
      })
      await slotNode.first().locator('.expander').click()

      // The "Id" child is a leaf with type ASSETGUID — rendered as a .guid-link button
      const guidLink = page.locator(`.tree button.guid-link[data-guid="${equip.guid}"]`)
      await guidLink.waitFor({ timeout: 5_000 })

      // Click the GUID link → switches to Catalog tab and triggers openDetail
      await guidLink.click()

      // Verify we switched to the Catalog tab
      await expect(page.locator('button.tab.active')).toHaveText('Catalog')

      // Wait for the detail pane to appear (catalog.db ~23MB loads first time)
      const detailPane = page.locator('.detail-pane')
      await detailPane.waitFor({ timeout: 10_000 })

      // Verify the resolved asset name
      await expect(detailPane.locator('h2.detail-name')).toContainText(equip.name, {
        timeout: 5_000,
      })
      await expect(detailPane).toContainText('Catalog Version')
      await expect(detailPane).toContainText('v2')
      await expect(detailPane.locator('a.detail-link')).toHaveAttribute('href', /norestforthewicked\.gg\/db\//)
    })
  }
})

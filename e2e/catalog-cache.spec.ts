import { expect, test } from '@playwright/test'

test.describe('Catalog cache', () => {
  test('reuses cached asset rows for repeated GUID lookups', async ({ page }) => {
    await page.goto('/')

    const result = await page.evaluate(async () => {
      const catalog = await import('/src/lib/catalog.ts')
      await catalog.loadCatalog()
      catalog.__resetCatalogTestState()

      const guid = catalog.searchCatalog('', 1)[0]?.guid ?? null
      if (!guid) {
        return {
          guid: null,
          firstName: null,
          secondName: null,
          sameReference: false,
          afterFirst: catalog.__getCatalogTestState(),
          afterSecond: catalog.__getCatalogTestState(),
        }
      }

      const first = catalog.getAssetByGuid(guid)
      const afterFirst = catalog.__getCatalogTestState()
      const second = catalog.getAssetByGuid(guid)
      const afterSecond = catalog.__getCatalogTestState()

      return {
        guid,
        firstName: first?.displayName ?? first?.name ?? null,
        secondName: second?.displayName ?? second?.name ?? null,
        sameReference: first === second,
        afterFirst,
        afterSecond,
      }
    })

    expect(result.guid).toBeTruthy()
    expect(result.firstName).toBeTruthy()
    expect(result.secondName).toBe(result.firstName)
    expect(result.sameReference).toBe(true)
    expect(result.afterFirst.assetQueryCount).toBe(1)
    expect(result.afterFirst.assetCacheSize).toBe(1)
    expect(result.afterSecond.assetQueryCount).toBe(1)
    expect(result.afterSecond.assetCacheSize).toBe(1)
  })
})

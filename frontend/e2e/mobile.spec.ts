import { test, expect, Page } from '@playwright/test'

test.use({ viewport: { width: 375, height: 812 }, hasTouch: true, isMobile: true })

const tabs = ['Templates', 'Photos', 'Receipt', 'Patterns', 'Weave', 'Editor', 'JSON API']

async function horizontalOverflow(page: Page) {
  return page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth)
}

test.describe('Mobile layout', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/')
    await page.locator('button.tab.active').waitFor()
  })

  for (const tab of tabs) {
    test(`${tab} tab does not scroll horizontally`, async ({ page }) => {
      await page.locator(`button.tab:has-text("${tab}")`).click()
      await expect(page.locator(`button.tab:has-text("${tab}")`)).toHaveClass(/active/)
      await expect(page.locator('.preview-stage > :first-child')).toBeVisible()
      expect(await horizontalOverflow(page)).toBe(0)
    })
  }

  test('every tab button is within the viewport', async ({ page }) => {
    const width = page.viewportSize()!.width
    for (const tab of tabs) {
      const box = await page.locator(`button.tab:has-text("${tab}")`).boundingBox()
      expect(box, tab).toBeTruthy()
      expect(box!.x, tab).toBeGreaterThanOrEqual(0)
      expect(box!.x + box!.width, tab).toBeLessThanOrEqual(width)
    }
  })

  test('preview fits the viewport width', async ({ page }) => {
    const width = page.viewportSize()!.width
    const box = await page.locator('.preview-container').boundingBox()
    expect(box).toBeTruthy()
    expect(box!.x).toBeGreaterThanOrEqual(0)
    expect(box!.x + box!.width).toBeLessThanOrEqual(width)
  })
})

test.describe('Mobile canvas overlay', () => {
  test('canvas elements can be dragged by touch', async ({ page }) => {
    await page.goto('/')
    await page.locator('button.tab:has-text("Editor")').tap()
    await expect(page.locator('.preview-image')).toBeVisible({ timeout: 15000 })

    const items = page.locator('.layer-item')
    await expect(items).toHaveCount(3)
    await items.nth(2).tap()
    const overlay = page.locator('.layer-overlay')
    await expect(overlay).toBeAttached({ timeout: 10000 })

    await page.locator('summary:has-text("Advanced")').tap()
    const textarea = page.locator('.json-editor textarea')
    const initialPos = JSON.parse(await textarea.inputValue()).document[2].elements[3].position

    await overlay.scrollIntoViewIfNeeded()
    const bounds = (await overlay.boundingBox())!
    await page.touchscreen.tap(bounds.x + bounds.width / 2, bounds.y + bounds.height / 2)
    const box = overlay.locator('.layer-box').nth(3)
    await expect(box).toBeAttached({ timeout: 3000 })

    const b = (await box.boundingBox())!
    const cx = b.x + b.width / 2
    const cy = b.y + b.height / 2
    const cdp = await page.context().newCDPSession(page)
    const touch = (type: string, x: number, y: number) =>
      cdp.send('Input.dispatchTouchEvent', {
        type,
        touchPoints: type === 'touchEnd' ? [] : [{ x, y }],
      })
    await touch('touchStart', cx, cy)
    for (let i = 1; i <= 5; i++) await touch('touchMove', cx + 8 * i, cy + 4 * i)
    await touch('touchEnd', cx + 40, cy + 20)

    await expect
      .poll(async () => JSON.parse(await textarea.inputValue()).document[2].elements[3].position?.x)
      .not.toBe(initialPos?.x)
  })
})

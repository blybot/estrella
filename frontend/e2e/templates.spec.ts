import { test, expect } from '@playwright/test'

// Uses examples/templates/notice.json (the e2e server runs with
// ESTRELLA_TEMPLATES_DIR=examples/templates).
test.describe('Templates tab', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/')
    await page.locator('button.tab.active').waitFor()
    await page.click('button.tab:has-text("Templates")')
  })

  test('lists templates and renders their fields', async ({ page }) => {
    await expect(page.locator('#template-select')).toHaveValue('notice')
    await expect(page.locator('label[for="template-field-title"]')).toHaveText('Title *')
    await expect(page.locator('textarea#template-field-message')).toBeVisible()
    await expect(page.locator('input#template-field-link')).toHaveAttribute('type', 'url')
  })

  test('previews the filled template', async ({ page }) => {
    await page.fill('#template-field-title', 'Hello')
    const preview = page.locator('.preview-image')
    await expect(preview).toBeVisible()
    await expect(preview).toHaveAttribute('src', /^blob:/)
  })

  test('shows field errors from the server', async ({ page }) => {
    await page.fill('#template-field-link', 'not a url')
    await expect(page.locator('.error-hint')).toHaveText('must be an http:// or https:// URL')
  })

  test('lists files that are not templates', async ({ page }) => {
    // examples/templates/README.md isn't a .json file
    await expect(page.locator('.hint li')).toContainText('README.md: only .json files are templates')
  })

  test('reports required fields when printing', async ({ page }) => {
    // Skip the browser's own required check to exercise the server's.
    await page.locator('#template-field-title').waitFor()
    await page.evaluate(() => document.querySelector('form')?.setAttribute('novalidate', ''))
    await page.click('button[type="submit"]')
    await expect(page.locator('.error-hint')).toHaveText('required')
  })
})

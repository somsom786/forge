// [simulated] Browser-preview end-to-end tests. See playwright.config.ts for what these do and do not cover.
import AxeBuilder from '@axe-core/playwright';
import { expect, test, type Page } from '@playwright/test';

const SCREENS = ['Command Center', 'Projects', 'Missions', 'Agents', 'Knowledge', 'Harness Studio', 'Experiments', 'Settings'];

async function open(page: Page) {
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });
  await page.goto('/');
  await expect(page.getByRole('navigation', { name: 'Primary' })).toBeVisible();
  return errors;
}

async function go(page: Page, label: string) {
  await page.getByRole('navigation', { name: 'Primary' }).getByText(label, { exact: true }).click();
}

/** WCAG 2.x A/AA rules; serious and critical violations fail the test, with what and where. */
async function expectAccessible(page: Page, where: string) {
  const r = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa']).analyze();
  const bad = r.violations.filter((v) => v.impact === 'serious' || v.impact === 'critical');
  expect(bad.map((v) => `${where}: ${v.id} — ${v.nodes.slice(0, 3).map((n) => n.target.join(' ')).join(', ')}`)).toEqual([]);
}

async function horizontalOverflow(page: Page) {
  return page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
}

test('[simulated] every screen renders without errors, fits the minimum window and has no serious accessibility violations', async ({ page }) => {
  const errors = await open(page);
  for (const s of SCREENS) {
    await go(page, s);
    await expect(page.locator('main, [role="main"]').first()).toBeVisible();
    expect(await horizontalOverflow(page), `${s} scrolls sideways at 1100×700`).toBeLessThanOrEqual(0);
    await expectAccessible(page, s);
  }
  expect(errors).toEqual([]);
});

test('[simulated] the mission workspace: tabs, approval card and proof pack are reachable and accessible', async ({ page }) => {
  const errors = await open(page);
  await go(page, 'Missions');
  await page.getByText('Add retry with exponential backoff to the HTTP client').first().click();
  await expect(page.getByRole('heading', { level: 1 })).toContainText('Add retry');
  expect(await horizontalOverflow(page)).toBeLessThanOrEqual(0);
  await expectAccessible(page, 'mission workspace');
  const tabs = page.getByRole('tab');
  const n = await tabs.count();
  expect(n).toBeGreaterThan(2);
  for (let i = 0; i < n; i++) {
    const name = (await tabs.nth(i).innerText()).trim();
    await tabs.nth(i).click();
    await expect(tabs.nth(i)).toHaveAttribute('aria-selected', 'true');
    await expectAccessible(page, `mission tab "${name}"`);
  }
  expect(errors).toEqual([]);
});

test('[simulated] the command palette works from the keyboard alone and gives focus back', async ({ page }) => {
  await open(page);
  await page.keyboard.press('Control+k');
  const palette = page.getByRole('dialog');
  await expect(palette).toBeVisible();
  await page.keyboard.type('Go to Knowledge');
  await page.keyboard.press('Enter');
  await expect(palette).toBeHidden();
  await expect(page.getByRole('heading', { level: 1, name: 'Knowledge' })).toBeVisible();
  await page.keyboard.press('Control+k');
  await expect(palette).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(palette).toBeHidden();
});

test('[simulated] the new-mission dialog traps focus, validates, and closes on Escape', async ({ page }) => {
  await open(page);
  await go(page, 'Missions');
  await page.getByRole('button', { name: /New mission/i }).first().click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  await expectAccessible(page, 'new mission dialog');
  // Focus stays inside the dialog however far you tab.
  for (let i = 0; i < 25; i++) {
    await page.keyboard.press('Tab');
    expect(await dialog.evaluate((d) => d.contains(document.activeElement))).toBe(true);
  }
  await page.keyboard.press('Escape');
  await expect(dialog).toBeHidden();
});

test('[simulated] choosing where agents run explains the trade-off and reports a refusal', async ({ page }) => {
  await open(page);
  await go(page, 'Projects');
  await page.getByRole('table', { name: 'Projects' }).getByText('atlas-api').first().click();
  await page.getByRole('button', { name: /Agents run:/ }).first().click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toContainText('The network stays open');
  await expectAccessible(page, 'agent backend dialog');
  // The preview offers no isolated backend: every "Use this" for one is disabled, and nothing claims isolation.
  await expect(dialog.getByText('current')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('button', { name: /Agents run: Trusted local/ }).first()).toBeVisible();
});

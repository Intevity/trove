import { expect, test } from '@playwright/test';

test('renders the Hello, Trove header', async ({ page }) => {
  await page.goto('/');

  const header = page.getByTestId('app-header');
  await expect(header).toBeVisible();
  await expect(header).toHaveText('Hello, Trove');

  await page.screenshot({ path: 'playwright-report/launch.png', fullPage: true });
});

test('shows the MVP harness count from @trove/shared', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByText(/Targeting/i)).toBeVisible();
});

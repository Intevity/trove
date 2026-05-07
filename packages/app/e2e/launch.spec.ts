import { expect, test } from '@playwright/test';

test('renders the Trove header', async ({ page }) => {
  await page.goto('/');

  const header = page.getByTestId('app-header');
  await expect(header).toBeVisible();
  await expect(header).toHaveText('Trove');

  await page.screenshot({ path: 'playwright-report/launch.png', fullPage: true });
});

test('mounts the harness-list region', async ({ page }) => {
  await page.goto('/');
  // The Vite dev server has no Tauri runtime, so `invoke` rejects and the
  // hook surfaces the structured error banner. Sprint 6 will switch this
  // suite to a tauri-driver harness with a real backend; until then the
  // banner's presence proves the App component mounted and the IPC client
  // wired up correctly.
  const errorBanner = page.getByTestId('harness-list-error');
  await expect(errorBanner).toBeVisible();
});

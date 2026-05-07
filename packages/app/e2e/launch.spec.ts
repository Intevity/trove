import { expect, test } from '@playwright/test';

test('renders the Trove header', async ({ page }) => {
  await page.goto('/');

  const header = page.getByTestId('app-header');
  await expect(header).toBeVisible();
  await expect(header).toHaveText('Trove');

  await page.screenshot({ path: 'playwright-report/launch.png', fullPage: true });
});

test('mounts the wizard region when no backend is loaded', async ({ page }) => {
  await page.goto('/');
  // The Vite dev server has no Tauri runtime, so `invoke` rejects and
  // `useAppState` settles with `appState === null`. App.tsx routes
  // that case to the first-run wizard. Sprint 6 will switch this suite
  // to a tauri-driver harness with real IPC; until then the wizard's
  // presence proves the App component mounted and the IPC client wired
  // up correctly.
  const wizard = page.getByTestId('backend-wizard');
  await expect(wizard).toBeVisible();
  // The preset picker is the wizard's first step; SigNoz is pinned to
  // the top by `@trove/collector-presets`.
  const signoz = page.getByTestId('preset-signoz');
  await expect(signoz).toBeVisible();
});

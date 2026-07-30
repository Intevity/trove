import { expect, test } from '@playwright/test';

import { installTauriMock } from './helpers/tauri-mock.js';

// These specs originally ran with no Tauri shim at all and leaned on `invoke`
// rejecting: the app used to treat a failed state load as a first run and drop
// straight into the wizard. App.tsx deliberately stopped doing that — a load
// failure now renders StateRecoveryNotice, because showing the wizard would
// imply the user's configuration had been wiped. Under the bare Vite dev
// server both specs therefore landed on the recovery screen and failed looking
// for elements that were never going to mount.
//
// They now install the same mocked Tauri runtime the other e2e specs use,
// seeded with a valid state carrying no backends — which is the real first-run
// path this suite is meant to cover.

test('renders the Trove header', async ({ page }) => {
  await installTauriMock(page);
  await page.goto('/');

  const header = page.getByTestId('app-header');
  await expect(header).toBeVisible();
  await expect(header).toHaveText('Trove');

  await page.screenshot({ path: 'playwright-report/launch.png', fullPage: true });
});

test('mounts the wizard region when no backend is loaded', async ({ page }) => {
  // DEFAULT_MOCK_STATE seeds `backends: []`, so App.tsx routes to the
  // first-run wizard.
  await installTauriMock(page);
  await page.goto('/');

  const wizard = page.getByTestId('backend-wizard');
  await expect(wizard).toBeVisible();
  // The preset picker is the wizard's first step; SigNoz is pinned to
  // the top by `@trove/collector-presets`.
  const signoz = page.getByTestId('preset-signoz');
  await expect(signoz).toBeVisible();
});

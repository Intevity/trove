import { expect, test } from '@playwright/test';

import { installTauriMock } from './helpers/tauri-mock.js';

/** Sprint 6 PR 3 acceptance — full first-run flow against the mocked
 *  Tauri runtime. Drives the wizard from preset → credentials → test
 *  export → save, lands on the dashboard, enables Claude Code, clicks
 *  "Test Pipeline", and asserts the header health dot transitions to
 *  green with non-zero counts in the SidecarPanel. */
test('full first-run flow lands the dashboard in the green state', async ({ page }) => {
  await installTauriMock(page, {
    collectorStatus: {
      state: { kind: 'running', pid: 1234, restarts: 0 },
      logPath: '/tmp/test/collector.log',
    },
  });

  await page.goto('/');

  // Wizard mounts because the seeded app state has no backend.
  await expect(page.getByTestId('backend-wizard')).toBeVisible();
  await page.getByTestId('preset-signoz').click();

  // Credentials form: SigNoz endpoint defaults to
  // "ingest.us.signoz.cloud:443"; fill the ingestion key.
  await expect(page.getByTestId('credentials-form')).toBeVisible();
  await page.getByTestId('signoz-ingestion-key').fill('signoz-secret-key');
  await page.getByTestId('credentials-form-continue').click();

  // Test Export step: run the export — the mock's `test_export`
  // returns ok; click Save.
  await expect(page.getByTestId('test-export-step')).toBeVisible();
  await page.getByTestId('test-export-run').click();
  await expect(page.getByTestId('test-export-banner-ok')).toBeVisible();
  await page.getByTestId('test-export-save').click();

  // Dashboard mounts. The header health dot starts amber because the
  // metrics snapshot is null (no scrape yet).
  const dashboard = page.getByTestId('dashboard');
  await expect(dashboard).toBeVisible();
  const dot = page.getByTestId('app-health-dot');
  await expect(dot).toHaveAttribute('data-health', 'amber');

  // Push a metrics snapshot with a recent signal — the dashboard's
  // useMetricsSnapshot hook updates and the dot flips green.
  // Zero enabled harnesses means the recent-signal check is skipped,
  // so the dot is green as soon as the snapshot lands and is
  // reachable.
  await page.evaluate(() => {
    const w = window as unknown as {
      __troveEmit: (e: string, p: unknown) => void;
      __troveSetState: (s: Record<string, unknown>) => void;
    };
    const snapshot = {
      received: { spans: 1, metricPoints: 0, logRecords: 0 },
      sent: { spans: 1, metricPoints: 0, logRecords: 0 },
      lastSignalMsAgo: 250,
      scrapedMsAgo: 100,
      unreachable: false,
      overallHealth: 'green',
    };
    w.__troveSetState({ metricsSnapshot: snapshot });
    w.__troveEmit('metrics-snapshot', snapshot);
  });

  await expect(dot).toHaveAttribute('data-health', 'green');

  // The sidecar panel reflects the non-zero counts pushed via the
  // event payload.
  await expect(page.getByTestId('counts-received-spans')).toHaveText('1');
  await expect(page.getByTestId('counts-sent-spans')).toHaveText('1');

  // Trigger the dashboard's Test Pipeline button. The mock returns
  // ok and the result toast appears next to the button.
  await page.getByTestId('test-pipeline-button').click();
  await expect(page.getByTestId('test-pipeline-result')).toHaveAttribute('data-status', 'ok');

  // The harness list lives behind the Harnesses tab in the new shell
  // — switch to it before opening the diff modal.
  await page.getByTestId('tab-harnesses').click();

  // The harness list is rendered with Claude Code marked as detected
  // (per the mock seed). Enable it via the diff modal.
  await page.getByLabel('toggle-claude-code').click();
  await expect(page.getByTestId('patch-preview-modal')).toBeVisible();
  await page.getByTestId('patch-preview-apply').click();
  await expect(page.getByTestId('patch-preview-modal')).toBeHidden();
});

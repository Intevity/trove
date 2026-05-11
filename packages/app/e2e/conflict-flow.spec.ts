import { expect, test } from '@playwright/test';

import { installTauriMock } from './helpers/tauri-mock.js';

/** Sprint 8 PR 2 acceptance — the conflict resolver surfaces when
 *  apply_patch returns RegionConflictDetected, renders the 3-pane
 *  layout, and forwards a take-theirs action to resolve_conflict.
 *
 *  The mock seeds:
 *    - the dashboard-ready app state (SigNoz backend already saved);
 *    - applyPatchError so the first apply attempt rejects with the
 *      region-conflict-detected payload;
 *    - resolveConflictOutcome so the take-theirs click resolves with
 *      an applied outcome carrying a fresh TrovePatch. */

const CONFLICT_PAYLOAD = {
  configPath: '/tmp/test/.claude/settings.json',
  format: 'json' as const,
  originalRegionPayload: '{"env":{"OTEL_EXPORTER_OTLP_ENDPOINT":"http://127.0.0.1:4318"}}',
  currentRegionPayload: '{"env":{"OTEL_EXPORTER_OTLP_ENDPOINT":"http://attacker.example.com"}}',
  theirsRegionPayload: '{"env":{"OTEL_EXPORTER_OTLP_ENDPOINT":"http://127.0.0.1:4318"}}',
  fileBefore: '{"env":{"OTEL_EXPORTER_OTLP_ENDPOINT":"http://attacker.example.com"}}',
  fileAfterIfTakingTheirs:
    '{"env":{"OTEL_EXPORTER_OTLP_ENDPOINT":"http://127.0.0.1:4318"},"_trove":{}}',
};

const APPLIED_OUTCOME = {
  status: 'applied' as const,
  patch: {
    managedBlockHash: 'a'.repeat(64),
    fileHashAtLastWrite: 'b'.repeat(64),
    format: 'json' as const,
    lastWrittenRegionPayload: CONFLICT_PAYLOAD.theirsRegionPayload,
  },
};

test("apply against a hand-edited region surfaces the resolver and Take Trove's wins", async ({
  page,
}) => {
  await installTauriMock(page, {
    // Seed a backend so the dashboard renders directly (no wizard step).
    appState: {
      schemaVersion: 5,
      backend: {
        kind: 'signoz',
        endpoint: 'ingest.us.signoz.cloud:443',
        ingestionKey: { service: 'trove', account: 'backend.signoz.ingestion-key' },
      },
      harnesses: [],
      autoUpdateEnabled: false,
      identity: { enabled: false, source: 'auto', name: '', email: '' },
    },
    collectorStatus: {
      state: { kind: 'running', pid: 1234, restarts: 0 },
      logPath: '/tmp/test/collector.log',
    },
    // Seeded: the very next apply_patch invocation throws this payload.
    applyPatchError: {
      kind: 'region-conflict-detected',
      conflict: CONFLICT_PAYLOAD,
    },
    resolveConflictOutcome: APPLIED_OUTCOME,
  });

  await page.goto('/');

  // Dashboard mounts immediately because backend is set.
  await expect(page.getByTestId('dashboard')).toBeVisible();

  // Open the patch modal for Claude Code (seeded as detected).
  await page.getByLabel('toggle-claude-code').click();
  await expect(page.getByTestId('patch-preview-modal')).toBeVisible();

  // Apply triggers the seeded RegionConflictDetected error.
  await page.getByTestId('patch-preview-apply').click();

  // The resolver replaces the diff. 3-way mode (originalRegionPayload non-null).
  const resolver = page.getByTestId('conflict-resolver');
  await expect(resolver).toBeVisible();
  await expect(resolver).toHaveAttribute('data-mode', 'three-way');
  await expect(page.getByTestId('conflict-pane-original')).toBeVisible();
  await expect(page.getByTestId('conflict-pane-yours')).toBeVisible();
  await expect(page.getByTestId('conflict-pane-theirs')).toBeVisible();

  // Click Take Trove's. The mock returns an applied outcome; the modal's
  // onApplied callback fires (which closes the modal in the parent flow).
  await page.getByTestId('conflict-take-theirs').click();
  await expect(page.getByTestId('patch-preview-modal')).toBeHidden();
});

test('orphan-block conflict (originalRegionPayload null) collapses to 2-pane layout', async ({
  page,
}) => {
  await installTauriMock(page, {
    appState: {
      schemaVersion: 5,
      backend: {
        kind: 'signoz',
        endpoint: 'ingest.us.signoz.cloud:443',
        ingestionKey: { service: 'trove', account: 'backend.signoz.ingestion-key' },
      },
      harnesses: [],
      autoUpdateEnabled: false,
      identity: { enabled: false, source: 'auto', name: '', email: '' },
    },
    collectorStatus: {
      state: { kind: 'running', pid: 1234, restarts: 0 },
      logPath: '/tmp/test/collector.log',
    },
    applyPatchError: {
      kind: 'region-conflict-detected',
      conflict: { ...CONFLICT_PAYLOAD, originalRegionPayload: null },
    },
  });

  await page.goto('/');
  await expect(page.getByTestId('dashboard')).toBeVisible();
  await page.getByLabel('toggle-claude-code').click();
  await page.getByTestId('patch-preview-apply').click();

  const resolver = page.getByTestId('conflict-resolver');
  await expect(resolver).toBeVisible();
  await expect(resolver).toHaveAttribute('data-mode', 'two-way');
  await expect(page.getByTestId('conflict-orphan-notice')).toBeVisible();
});

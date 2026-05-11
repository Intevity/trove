import type { BackendDraft, TestExportResult } from '@trove/shared';

import { SyntheticSpanHints } from './SyntheticSpanHints.js';

export interface TestExportStepProps {
  /** Disables every button while we're mid-test or mid-save. */
  busy: boolean;
  /** What the most recent run returned, or null when the user hasn't
   *  pressed the button yet. */
  result: TestExportResult | null;
  /** Kind of backend the user is configuring; threads through to the
   *  post-success hints block so the "Look for this in {label}"
   *  preamble names the user's chosen backend by label. */
  backendKind?: BackendDraft['kind'] | undefined;
  onTest: () => void;
  onSave: () => void;
  onBack: () => void;
}

export function TestExportStep({
  busy,
  result,
  backendKind,
  onTest,
  onSave,
  onBack,
}: TestExportStepProps): JSX.Element {
  const status = result?.status ?? null;
  const canSave = status === 'ok' && !busy;
  const showSaveAnyway = (status === 'failed' || status === 'timeout') && !busy;

  return (
    <section data-testid="test-export-step">
      <h2 className="text-xl font-semibold text-slate-900 dark:text-slate-100">
        Send a synthetic span
      </h2>
      <p className="mt-1 text-sm text-slate-600 dark:text-slate-400">
        Trove will send one synthetic OTLP trace through the local collector to your backend, then
        watch for any error in the collector log within five seconds. Click <strong>Save</strong>{' '}
        once you see a green check.
      </p>

      <div className="mt-6 space-y-4">
        <button
          type="button"
          onClick={onTest}
          disabled={busy}
          data-testid="test-export-run"
          className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white shadow-sm hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-blue-500 disabled:bg-slate-400"
        >
          {busy && status === null ? 'Testing…' : result === null ? 'Test export' : 'Test again'}
        </button>

        {result ? <ResultBanner result={result} backendKind={backendKind} /> : null}

        <div className="flex items-center justify-between pt-2">
          <button
            type="button"
            onClick={onBack}
            disabled={busy}
            data-testid="test-export-back"
            className="text-sm text-slate-600 hover:text-slate-900 disabled:text-slate-400 dark:text-slate-400 dark:hover:text-slate-100"
          >
            ← Back
          </button>

          <div className="flex items-center gap-3">
            {showSaveAnyway ? (
              <button
                type="button"
                onClick={onSave}
                data-testid="test-export-save-anyway"
                className="text-sm text-slate-600 underline hover:text-slate-900 dark:text-slate-400 dark:hover:text-slate-100"
              >
                Save anyway
              </button>
            ) : null}
            <button
              type="button"
              onClick={onSave}
              disabled={!canSave}
              data-testid="test-export-save"
              className="rounded-md bg-emerald-600 px-4 py-2 text-sm font-medium text-white shadow-sm hover:bg-emerald-700 focus:outline-none focus:ring-2 focus:ring-emerald-500 disabled:bg-slate-300 disabled:text-slate-600"
            >
              Save
            </button>
          </div>
        </div>
      </div>
    </section>
  );
}

function ResultBanner({
  result,
  backendKind,
}: {
  result: TestExportResult;
  backendKind?: BackendDraft['kind'] | undefined;
}): JSX.Element {
  if (result.status === 'ok') {
    return (
      <div data-testid="test-export-banner-ok">
        <p className="rounded-md border border-emerald-300 bg-emerald-50 px-3 py-2 text-sm text-emerald-900 dark:border-emerald-700 dark:bg-emerald-950 dark:text-emerald-200">
          ✅ {result.detail}
        </p>
        <SyntheticSpanHints backendKind={backendKind} />
      </div>
    );
  }
  if (result.status === 'failed') {
    return (
      <p
        data-testid="test-export-banner-failed"
        className="rounded-md border border-red-300 bg-red-50 px-3 py-2 text-sm text-red-900 dark:border-red-700 dark:bg-red-950 dark:text-red-200"
      >
        ❌ {result.detail}
      </p>
    );
  }
  // timeout
  return (
    <p
      data-testid="test-export-banner-timeout"
      className="rounded-md border border-amber-300 bg-amber-50 px-3 py-2 text-sm text-amber-900 dark:border-amber-700 dark:bg-amber-950 dark:text-amber-200"
    >
      ⏱ {result.detail}
    </p>
  );
}

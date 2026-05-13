import type { BackendDraft, TestExportResult } from '@trove/shared';

import { Button, StatusDot } from '../ui/index.js';
import { SyntheticSpanHints } from './SyntheticSpanHints.js';

export interface TestExportStepProps {
  busy: boolean;
  result: TestExportResult | null;
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
      <h2 className="text-[20px] font-semibold tracking-tight text-fg-primary dark:text-fg-primary-dark">
        Send a synthetic span
      </h2>
      <p className="mt-1 text-[13px] text-fg-secondary dark:text-fg-secondary-dark">
        Trove will send one synthetic OTLP trace through the local collector to your backend, then
        watch for any error in the collector log within five seconds. Click <strong>Save</strong>{' '}
        once you see a green check.
      </p>

      <div className="mt-6 space-y-4">
        <Button
          variant="primary"
          size="md"
          testid="test-export-run"
          onClick={onTest}
          disabled={busy}
        >
          {busy && status === null ? 'Testing…' : result === null ? 'Test export' : 'Test again'}
        </Button>

        {result ? <ResultBanner result={result} backendKind={backendKind} /> : null}

        <div className="flex items-center justify-between pt-2">
          <Button
            variant="ghost"
            size="md"
            testid="test-export-back"
            onClick={onBack}
            disabled={busy}
          >
            ← Back
          </Button>

          <div className="flex items-center gap-3">
            {showSaveAnyway ? (
              <Button variant="ghost" size="sm" testid="test-export-save-anyway" onClick={onSave}>
                Save anyway
              </Button>
            ) : null}
            <Button
              variant="primary"
              size="md"
              testid="test-export-save"
              onClick={onSave}
              disabled={!canSave}
            >
              Save
            </Button>
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
        <div className="flex items-start gap-2 rounded-card border border-hairline bg-ios-green/[0.08] px-3 py-2 text-[13px] text-fg-primary dark:border-hairline-dark dark:text-fg-primary-dark">
          <StatusDot status="green" size="md" pulse={false} className="mt-1" />
          <span>{result.detail}</span>
        </div>
        <SyntheticSpanHints backendKind={backendKind} />
      </div>
    );
  }
  if (result.status === 'failed') {
    return (
      <div
        data-testid="test-export-banner-failed"
        className="flex items-start gap-2 rounded-card border border-hairline bg-ios-red/[0.08] px-3 py-2 text-[13px] text-fg-primary dark:border-hairline-dark dark:text-fg-primary-dark"
      >
        <StatusDot status="red" size="md" pulse={false} className="mt-1" />
        <span>{result.detail}</span>
      </div>
    );
  }
  return (
    <div
      data-testid="test-export-banner-timeout"
      className="flex items-start gap-2 rounded-card border border-hairline bg-ios-orange/[0.08] px-3 py-2 text-[13px] text-fg-primary dark:border-hairline-dark dark:text-fg-primary-dark"
    >
      <StatusDot status="amber" size="md" pulse={false} className="mt-1" />
      <span>{result.detail}</span>
    </div>
  );
}

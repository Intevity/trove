import { useCallback, useState } from 'react';

import type { BackendDraft, TestExportResult } from '@trove/shared';

import { TroveIpcError, saveBackend, testExport } from '../../lib/ipc.js';
import { CredentialsForm, type Kind } from './CredentialsForm.js';
import { PresetPicker } from './PresetPicker.js';
import { TestExportStep } from './TestExportStep.js';

type Step = 'pick-preset' | 'enter-creds' | 'test-export';

export interface BackendWizardProps {
  /** Called after the user successfully saves a backend. The parent
   *  swaps the wizard out for the dashboard view. */
  onComplete: () => void;
}

/** First-run wizard for picking a destination backend. Step machine:
 *
 * - `pick-preset`: choose between SigNoz / Honeycomb / Grafana Cloud /
 *   Datadog / generic OTLP / passthrough.
 * - `enter-creds`: per-kind form, secret fields masked.
 * - `test-export`: synthetic span flows through the local collector;
 *   on green the Save button enables; on red a "Save anyway" link
 *   appears for users behind corporate proxies.
 *
 * The wizard owns its state via plain useState — no form library — to
 * mirror the rest of the Trove app. Mounted only when
 * `appState.backend === null`. */
export function BackendWizard({ onComplete }: BackendWizardProps): JSX.Element {
  const [step, setStep] = useState<Step>('pick-preset');
  const [kind, setKind] = useState<Kind | null>(null);
  const [draft, setDraft] = useState<BackendDraft | null>(null);
  const [testResult, setTestResult] = useState<TestExportResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handlePresetSelect = useCallback((next: Kind) => {
    setKind(next);
    setStep('enter-creds');
    setError(null);
  }, []);

  const handleCredsSubmit = useCallback((next: BackendDraft) => {
    setDraft(next);
    setTestResult(null);
    setStep('test-export');
    setError(null);
  }, []);

  const handleBackToPicker = useCallback(() => {
    setStep('pick-preset');
    setKind(null);
    setDraft(null);
    setTestResult(null);
    setError(null);
  }, []);

  const handleBackToCreds = useCallback(() => {
    setStep('enter-creds');
    setError(null);
  }, []);

  const handleTest = useCallback(async () => {
    if (!draft) return;
    setBusy(true);
    setError(null);
    try {
      // The wizard saves the draft *before* testing — test_export needs
      // the supervisor to be running with the user's chosen backend.
      // If the test goes green, the user clicks Save (which is a no-op
      // beyond closing the wizard). If red, the user can iterate by
      // editing creds (back to step 2) or accept the result (Save
      // anyway). Either way the backend persists; clear_backend reverts.
      await saveBackend(draft);
      const result = await testExport();
      setTestResult(result);
    } catch (e) {
      const message =
        e instanceof TroveIpcError
          ? `${e.cause.kind}: ${describeIpcError(e)}`
          : e instanceof Error
            ? e.message
            : String(e);
      setError(message);
      setTestResult(null);
    } finally {
      setBusy(false);
    }
  }, [draft]);

  const handleSave = useCallback(() => {
    // saveBackend already ran during the test; nothing to persist here.
    // Just hand control back to the dashboard.
    onComplete();
  }, [onComplete]);

  return (
    <div
      className="rounded-lg border border-slate-200 bg-white p-6 shadow-sm dark:border-slate-800 dark:bg-slate-900"
      data-testid="backend-wizard"
    >
      {step === 'pick-preset' ? <PresetPicker onSelect={handlePresetSelect} /> : null}
      {step === 'enter-creds' && kind !== null ? (
        <CredentialsForm
          key={kind}
          kind={kind}
          onSubmit={handleCredsSubmit}
          onBack={handleBackToPicker}
        />
      ) : null}
      {step === 'test-export' ? (
        <TestExportStep
          busy={busy}
          result={testResult}
          backendKind={draft?.kind ?? kind ?? undefined}
          onTest={() => void handleTest()}
          onSave={handleSave}
          onBack={handleBackToCreds}
        />
      ) : null}

      {error ? (
        <p
          data-testid="backend-wizard-error"
          className="mt-4 rounded-md border border-red-300 bg-red-50 px-3 py-2 text-sm text-red-900 dark:border-red-700 dark:bg-red-950 dark:text-red-200"
        >
          {error}
        </p>
      ) : null}
    </div>
  );
}

function describeIpcError(err: TroveIpcError): string {
  const cause = err.cause;
  switch (cause.kind) {
    case 'config-unparseable':
    case 'io':
      return cause.reason;
    case 'region-conflict':
      return cause.path;
    case 'region-conflict-detected':
      return cause.conflict.configPath;
    case 'harness-not-detected':
    case 'harness-not-implemented':
      return cause.id;
    case 'updater-check-failed':
    case 'internal':
      return cause.reason;
  }
}

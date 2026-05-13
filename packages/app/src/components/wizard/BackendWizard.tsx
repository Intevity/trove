import { useCallback, useState } from 'react';

import type { BackendDraft, TestExportResult } from '@trove/shared';

import { TroveIpcError, saveBackend, testExport } from '../../lib/ipc.js';
import { Card, StatusDot } from '../ui/index.js';
import { CredentialsForm, type Kind } from './CredentialsForm.js';
import { PresetPicker } from './PresetPicker.js';
import { TestExportStep } from './TestExportStep.js';

type Step = 'pick-preset' | 'enter-creds' | 'test-export';

export interface BackendWizardProps {
  onComplete: () => void;
}

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
    onComplete();
  }, [onComplete]);

  return (
    <Card as="div" padding="lg" testid="backend-wizard">
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
        <div
          data-testid="backend-wizard-error"
          className="mt-4 flex items-start gap-2 rounded-card border border-hairline bg-ios-red/[0.08] px-3 py-2 text-[13px] text-fg-primary dark:border-hairline-dark dark:text-fg-primary-dark"
        >
          <StatusDot status="red" size="md" pulse={false} className="mt-1" />
          <span>{error}</span>
        </div>
      ) : null}
    </Card>
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

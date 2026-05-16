import { useCallback, useState } from 'react';

import type { BackendDraft, BackendInstance, TestExportResult } from '@trove/shared';

import { TroveIpcError, addBackend, testExport, updateBackend } from '../../lib/ipc.js';
import { Card, StatusDot } from '../ui/index.js';
import { CredentialsForm, type Kind } from './CredentialsForm.js';
import { PresetPicker } from './PresetPicker.js';
import { TestExportStep } from './TestExportStep.js';

type Step = 'pick-preset' | 'enter-creds' | 'test-export';

/** "Add a new platform" vs "Edit an existing instance." In edit mode
 *  the preset picker is skipped (kind is fixed) and the credentials
 *  form is pre-seeded from the instance's non-secret fields; the user
 *  still re-types every secret because the keychain values are never
 *  read back into JS. In add mode, an optional `presetKind` likewise
 *  skips the picker — used when the Platforms list launches the wizard
 *  for a specific row. */
export type WizardMode =
  | { kind: 'add'; presetKind?: Kind }
  | { kind: 'edit'; instance: BackendInstance };

export interface BackendWizardProps {
  /** Defaults to `{ kind: 'add' }` for the first-run wizard host. */
  mode?: WizardMode;
  onComplete: () => void;
}

export function BackendWizard({
  mode = { kind: 'add' },
  onComplete,
}: BackendWizardProps): JSX.Element {
  const isEdit = mode.kind === 'edit';
  const initialKind: Kind | null = isEdit ? mode.instance.backend.kind : (mode.presetKind ?? null);
  const skipPicker = initialKind !== null;

  const [step, setStep] = useState<Step>(skipPicker ? 'enter-creds' : 'pick-preset');
  const [kind, setKind] = useState<Kind | null>(initialKind);
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
    // In edit mode (or any flow that bypassed the picker) there's
    // nowhere to step back to — cancel instead.
    if (isEdit || skipPicker) {
      onComplete();
      return;
    }
    setStep('pick-preset');
    setKind(null);
    setDraft(null);
    setTestResult(null);
    setError(null);
  }, [isEdit, skipPicker, onComplete]);

  const handleBackToCreds = useCallback(() => {
    setStep('enter-creds');
    setError(null);
  }, []);

  const handleTest = useCallback(async () => {
    if (!draft) return;
    setBusy(true);
    setError(null);
    try {
      if (isEdit) {
        await updateBackend(mode.instance.id, draft, mode.instance.label);
      } else {
        await addBackend(draft);
      }
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
  }, [draft, isEdit, mode]);

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
          {...(isEdit ? { initialFields: nonSecretFieldsForEdit(mode.instance) } : {})}
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

/** Map a stored `BackendInstance` back to the non-secret fields the
 *  `CredentialsForm` accepts as a starting point in edit mode. Secrets
 *  are intentionally left blank — the user re-enters them. */
function nonSecretFieldsForEdit(instance: BackendInstance): Partial<BackendDraft> {
  switch (instance.backend.kind) {
    case 'signoz':
      return { kind: 'signoz', endpoint: instance.backend.endpoint };
    case 'honeycomb':
      return { kind: 'honeycomb', dataset: instance.backend.dataset };
    case 'grafana-cloud':
      return { kind: 'grafana-cloud', endpoint: instance.backend.endpoint };
    case 'datadog':
      return { kind: 'datadog', site: instance.backend.site };
    case 'otlp-generic':
      return {
        kind: 'otlp-generic',
        endpoint: instance.backend.endpoint,
        protocol: instance.backend.protocol,
        // Headers names carry over so the user only re-types each
        // header's secret value, not the header key.
        headers: Object.fromEntries(Object.keys(instance.backend.headers).map((k) => [k, ''])),
      };
    case 'otelcol-passthrough':
      return { kind: 'otelcol-passthrough', endpoint: instance.backend.endpoint };
    case 'new-relic':
      return { kind: 'new-relic', region: instance.backend.region };
    case 'splunk-observability':
      return { kind: 'splunk-observability', realm: instance.backend.realm };
    case 'dynatrace':
      return { kind: 'dynatrace', environmentUrl: instance.backend.environmentUrl };
    case 'elastic':
      return { kind: 'elastic', endpoint: instance.backend.endpoint };
    case 'opensearch':
      return { kind: 'opensearch', endpoint: instance.backend.endpoint };
    case 'openobserve':
      return {
        kind: 'openobserve',
        endpoint: instance.backend.endpoint,
        organization: instance.backend.organization,
      };
    case 'clickstack':
      return { kind: 'clickstack', endpoint: instance.backend.endpoint };
    case 'chronosphere':
      return { kind: 'chronosphere', tenant: instance.backend.tenant };
    case 'sentry':
      return { kind: 'sentry', endpoint: instance.backend.endpoint };
  }
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

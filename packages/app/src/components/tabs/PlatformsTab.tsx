import { useCallback, useState } from 'react';

import { presetMetadataFor } from '@trove/collector-presets';
import type { AppState, Backend, BackendInstance } from '@trove/shared';

import { TroveIpcError, removeBackend } from '../../lib/ipc.js';
import { Button, Card, CardHeader, CardTitle, StatusDot } from '../ui/index.js';
import { BackendWizard, type WizardMode } from '../wizard/BackendWizard.js';

interface Props {
  appState: AppState;
  /** Re-fetches `state.json` after a write so the list reflects the
   *  newly-persisted instance. Same callback the Dashboard threads
   *  through to MappingsTab/SettingsTab. */
  onAppStateRefresh: () => void | Promise<void>;
}

type WizardState = { open: false } | { open: true; mode: WizardMode };

/** Multi-platform management page. Renders one card per configured
 *  destination with Edit / Remove affordances, plus an "Add platform"
 *  button that hosts the [`BackendWizard`] in add or edit mode. */
export function PlatformsTab({ appState, onAppStateRefresh }: Props): JSX.Element {
  const [wizardState, setWizardState] = useState<WizardState>({ open: false });
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const openAdd = useCallback(() => {
    setError(null);
    setWizardState({ open: true, mode: { kind: 'add' } });
  }, []);

  const openEdit = useCallback((instance: BackendInstance) => {
    setError(null);
    setWizardState({ open: true, mode: { kind: 'edit', instance } });
  }, []);

  const closeWizard = useCallback(async () => {
    setWizardState({ open: false });
    await onAppStateRefresh();
  }, [onAppStateRefresh]);

  const handleRemove = useCallback(
    async (id: string) => {
      setBusyId(id);
      setError(null);
      try {
        await removeBackend(id);
        await onAppStateRefresh();
      } catch (e) {
        setError(e instanceof TroveIpcError ? `${e.cause.kind}` : String(e));
      } finally {
        setBusyId(null);
      }
    },
    [onAppStateRefresh],
  );

  if (wizardState.open) {
    return (
      <div className="flex flex-col gap-3 px-4 py-3">
        <BackendWizard mode={wizardState.mode} onComplete={() => void closeWizard()} />
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-3 px-4 py-3">
      <header className="flex items-center justify-between">
        <div>
          <h2 className="text-[15px] font-semibold tracking-tight text-fg-primary dark:text-fg-primary-dark">
            Platforms
          </h2>
          <p className="text-[12px] text-fg-secondary dark:text-fg-secondary-dark">
            Every signal is forwarded to every configured destination.
          </p>
        </div>
        <Button variant="primary" size="sm" testid="add-platform" onClick={openAdd}>
          Add platform
        </Button>
      </header>

      {error ? (
        <div
          data-testid="platforms-tab-error"
          className="flex items-start gap-2 rounded-card border border-hairline bg-ios-red/[0.08] px-3 py-2 text-[13px] text-fg-primary dark:border-hairline-dark dark:text-fg-primary-dark"
        >
          <StatusDot status="red" size="md" pulse={false} className="mt-1" />
          <span>{error}</span>
        </div>
      ) : null}

      {appState.backends.length === 0 ? (
        <Card testid="platforms-empty">
          <div className="flex flex-col items-start gap-2">
            <p className="text-[14px] font-medium text-fg-primary dark:text-fg-primary-dark">
              No platforms configured
            </p>
            <p className="text-[12px] text-fg-secondary dark:text-fg-secondary-dark">
              Telemetry is being received locally but not forwarded yet. Add a destination to start
              fanning out spans, metrics, and logs.
            </p>
            <Button variant="primary" size="md" onClick={openAdd}>
              Configure a platform
            </Button>
          </div>
        </Card>
      ) : (
        <ul className="flex flex-col gap-2" data-testid="platforms-list">
          {appState.backends.map((instance) => (
            <PlatformRow
              key={instance.id}
              instance={instance}
              busy={busyId === instance.id}
              onEdit={() => openEdit(instance)}
              onRemove={() => void handleRemove(instance.id)}
            />
          ))}
        </ul>
      )}
    </div>
  );
}

interface PlatformRowProps {
  instance: BackendInstance;
  busy: boolean;
  onEdit: () => void;
  onRemove: () => void;
}

function PlatformRow({ instance, busy, onEdit, onRemove }: PlatformRowProps): JSX.Element {
  const meta = presetMetadataFor(instance.backend.kind);
  return (
    <Card testid={`platform-row-${instance.id}`}>
      <CardHeader>
        <CardTitle>{instance.label ?? meta.label}</CardTitle>
        <div className="flex items-center gap-1">
          <Button variant="ghost" size="sm" onClick={onEdit} disabled={busy}>
            Edit
          </Button>
          <Button variant="ghost" size="sm" onClick={onRemove} disabled={busy}>
            {busy ? 'Removing…' : 'Remove'}
          </Button>
        </div>
      </CardHeader>
      <p className="text-[12px] text-fg-secondary dark:text-fg-secondary-dark">
        {meta.label}
        <BackendDetail backend={instance.backend} />
      </p>
    </Card>
  );
}

function BackendDetail({ backend }: { backend: Backend }): JSX.Element | null {
  switch (backend.kind) {
    case 'signoz':
      return (
        <span className="text-fg-tertiary dark:text-fg-tertiary-dark"> · {backend.endpoint}</span>
      );
    case 'honeycomb':
      return (
        <span className="text-fg-tertiary dark:text-fg-tertiary-dark">
          {' '}
          · dataset {backend.dataset}
        </span>
      );
    case 'datadog':
      return (
        <span className="text-fg-tertiary dark:text-fg-tertiary-dark"> · site {backend.site}</span>
      );
    case 'grafana-cloud':
    case 'otelcol-passthrough':
      return (
        <span className="text-fg-tertiary dark:text-fg-tertiary-dark"> · {backend.endpoint}</span>
      );
    case 'otlp-generic':
      return (
        <span className="text-fg-tertiary dark:text-fg-tertiary-dark">
          {' · '}
          {backend.protocol.toUpperCase()} · {backend.endpoint}
        </span>
      );
  }
}

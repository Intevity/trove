import { presetMetadataFor } from '@trove/collector-presets';
import type { AppState, Backend, CollectorRunState, MetricsSnapshotWire } from '@trove/shared';

import { DiagnosticsPanel } from '../Diagnostics/DiagnosticsPanel.js';
import { SidecarPanel } from '../SidecarPanel.js';

interface Props {
  appState: AppState;
  state: CollectorRunState | null;
  metrics: MetricsSnapshotWire | null;
  onChangeBackend: () => void;
}

export function OverviewTab({ appState, state, metrics, onChangeBackend }: Props): JSX.Element {
  return (
    <div className="flex flex-col gap-4 px-4 py-3">
      <DiagnosticsPanel appState={appState} state={state} metrics={metrics} />

      {appState.backend ? (
        <BackendBanner backend={appState.backend} onChange={onChangeBackend} />
      ) : null}

      <SidecarPanel state={state} metrics={metrics} backend={appState.backend} />
    </div>
  );
}

function BackendBanner({
  backend,
  onChange,
}: {
  backend: Backend;
  onChange: () => void;
}): JSX.Element {
  const meta = presetMetadataFor(backend.kind);
  return (
    <p
      data-testid="backend-banner"
      className="flex items-center justify-between rounded-md border border-slate-200 bg-slate-100 px-3 py-2 text-sm text-slate-800 dark:border-slate-800 dark:bg-slate-900 dark:text-slate-200"
    >
      <span>
        Forwarding to <span className="font-medium">{meta.label}</span>
        <BackendDetail backend={backend} />
      </span>
      <button
        type="button"
        onClick={onChange}
        data-testid="backend-banner-change"
        className="text-xs text-blue-700 hover:text-blue-900 dark:text-blue-300 dark:hover:text-blue-100"
      >
        Change
      </button>
    </p>
  );
}

function BackendDetail({ backend }: { backend: Backend }): JSX.Element | null {
  switch (backend.kind) {
    case 'signoz':
      return <span className="text-slate-500"> ({backend.endpoint})</span>;
    case 'honeycomb':
      return <span className="text-slate-500"> ({backend.dataset})</span>;
    case 'datadog':
      return <span className="text-slate-500"> ({backend.site})</span>;
    case 'grafana-cloud':
    case 'otlp-generic':
    case 'otelcol-passthrough':
      return null;
  }
}

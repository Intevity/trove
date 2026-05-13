import type { AppState, CollectorRunState, MetricsSnapshotWire } from '@trove/shared';

import { BackendBanner } from '../BackendBanner.js';
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
    <div className="flex flex-col gap-3 px-4 py-3">
      <DiagnosticsPanel appState={appState} state={state} metrics={metrics} />

      {appState.backend ? (
        <BackendBanner backend={appState.backend} onChange={onChangeBackend} />
      ) : null}

      <SidecarPanel state={state} metrics={metrics} backend={appState.backend} />
    </div>
  );
}

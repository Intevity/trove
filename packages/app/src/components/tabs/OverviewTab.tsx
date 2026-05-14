import { ArrowRight } from 'lucide-react';

import type { AppState, CollectorRunState, MetricsSnapshotWire } from '@trove/shared';

import { DiagnosticsPanel } from '../Diagnostics/DiagnosticsPanel.js';
import { FlowChart } from '../FlowChart.js';
import { SidecarPanel } from '../SidecarPanel.js';

interface Props {
  appState: AppState;
  state: CollectorRunState | null;
  metrics: MetricsSnapshotWire | null;
  /** Jump to the Platforms tab. Used by the empty-state nudge that
   *  appears when no destinations are configured. */
  onOpenPlatforms: () => void;
}

export function OverviewTab({ appState, state, metrics, onOpenPlatforms }: Props): JSX.Element {
  return (
    <div className="flex flex-col gap-3 px-4 py-3">
      <DiagnosticsPanel appState={appState} state={state} metrics={metrics} />

      {appState.backends.length === 0 ? (
        <button
          type="button"
          data-testid="configure-platform-nudge"
          onClick={onOpenPlatforms}
          className="flex w-full items-center justify-between gap-2 rounded-card border border-hairline bg-surface-elevated px-3 py-2 text-left text-[12px] text-fg-secondary transition-colors hover:bg-ios-blue/[0.06] dark:border-hairline-dark dark:bg-surface-elevated-dark dark:text-fg-secondary-dark dark:hover:bg-ios-blue/[0.12]"
        >
          <span>
            No platforms configured —{' '}
            <span className="font-medium text-fg-primary dark:text-fg-primary-dark">
              configure one
            </span>{' '}
            to start forwarding telemetry.
          </span>
          <ArrowRight size={14} className="text-ios-blue" />
        </button>
      ) : null}

      <SidecarPanel state={state} metrics={metrics} backends={appState.backends} />

      <FlowChart
        harnessCount={appState.harnesses.filter((h) => h.enabled).length}
        backends={appState.backends}
        metrics={metrics}
        state={state}
      />
    </div>
  );
}

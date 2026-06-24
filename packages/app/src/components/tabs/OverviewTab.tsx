import { ArrowRight } from 'lucide-react';

import type { AppState, CollectorRunState, HarnessId, MetricsSnapshotWire } from '@trove/shared';

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
  /** Detected sources that forward into Trove's collector without being
   *  managed harnesses (today: Sentinel when it's forwarding to Trove).
   *  Merged into the flow chart alongside enabled harnesses so the
   *  pipeline reflects every live source, not just the ones Trove patches. */
  passiveSources?: readonly { id: HarnessId }[];
}

export function OverviewTab({
  appState,
  state,
  metrics,
  onOpenPlatforms,
  passiveSources,
}: Props): JSX.Element {
  const enabledHarnesses = appState.harnesses.filter((h) => h.enabled).map((h) => ({ id: h.id }));
  const enabledIds = new Set(enabledHarnesses.map((h) => h.id));
  const flowHarnesses = [
    ...enabledHarnesses,
    ...(passiveSources ?? []).filter((s) => !enabledIds.has(s.id)),
  ];
  return (
    <div className="flex flex-col gap-3 px-4 py-3">
      <DiagnosticsPanel appState={appState} state={state} metrics={metrics} />

      {appState.backends.length === 0 ? (
        <button
          type="button"
          data-testid="configure-platform-nudge"
          onClick={onOpenPlatforms}
          className="flex w-full items-center justify-between gap-2 rounded-card border border-hairline bg-surface-elevated px-3 py-2 text-left text-[12px] text-fg-secondary transition-colors hover:bg-brand/[0.06] dark:border-hairline-dark dark:bg-surface-elevated-dark dark:text-fg-secondary-dark dark:hover:bg-brand/[0.12]"
        >
          <span>
            No platforms configured;{' '}
            <span className="font-medium text-fg-primary dark:text-fg-primary-dark">
              configure one
            </span>{' '}
            to start forwarding telemetry.
          </span>
          <ArrowRight size={14} className="text-brand" />
        </button>
      ) : null}

      <FlowChart
        harnesses={flowHarnesses}
        backends={appState.backends.filter((b) => b.enabled)}
        metrics={metrics}
        state={state}
      />

      <SidecarPanel
        state={state}
        metrics={metrics}
        backends={appState.backends.filter((b) => b.enabled)}
      />
    </div>
  );
}

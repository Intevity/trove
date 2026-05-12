import { useCallback, useMemo, useState } from 'react';

import {
  type AppState,
  type HarnessId,
  type HarnessMapping,
  type MappingSource,
  type TierAMetric,
} from '@trove/shared';

import { TroveIpcError, applyMappings, resetMappingsToDefaults } from '../../lib/ipc.js';

interface Props {
  appState: AppState;
  onAppStateRefresh: () => void | Promise<void>;
}

/** Sprint 13 — Tier A mapping viewer.
 *
 *  Read-only display of every harness's mapping rows plus a master
 *  enable/disable toggle and a "reset to defaults" affordance. Editing
 *  individual rows lands in a follow-up PR (per the implementation
 *  plan); v1 is sufficient to give users visibility into what Trove is
 *  synthesizing on their behalf.
 *
 *  Hook-rule rows are documentation today — the existing hook/watcher
 *  drivers (Cursor, Cline, Aider, Copilot) hardcode their emission
 *  shape. The Mappings tab surfaces what they're doing so a user can
 *  audit it. The master enable toggle still works: flipping it off
 *  disables the harness's collector-side Tier A synthesis (for native-
 *  OTel harnesses) and is persisted alongside the other rows.
 *
 *  Native-OTel rows are the load-bearing ones: toggling a harness
 *  on/off regenerates `collector.yaml` to add/remove its
 *  `metricstransform/tierA-<harness>` processor. The collector
 *  recycle is automatic; the user sees a 1–3 second blip in the
 *  sidecar status while the new config takes effect. */
export function MappingsTab({ appState, onAppStateRefresh }: Props): JSX.Element {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<TroveIpcError | null>(null);

  const harnesses = appState.mappings.harnesses;

  const setHarnessEnabled = useCallback(
    async (id: HarnessId, enabled: boolean): Promise<void> => {
      setBusy(true);
      setError(null);
      try {
        const next = {
          ...appState.mappings,
          harnesses: appState.mappings.harnesses.map((h) =>
            h.harnessId === id ? { ...h, enabled } : h,
          ),
        };
        await applyMappings(next);
        await onAppStateRefresh();
      } catch (err) {
        if (err instanceof TroveIpcError) setError(err);
      } finally {
        setBusy(false);
      }
    },
    [appState.mappings, onAppStateRefresh],
  );

  const handleResetAll = useCallback(async (): Promise<void> => {
    setBusy(true);
    setError(null);
    try {
      await resetMappingsToDefaults();
      await onAppStateRefresh();
    } catch (err) {
      if (err instanceof TroveIpcError) setError(err);
    } finally {
      setBusy(false);
    }
  }, [onAppStateRefresh]);

  return (
    <div data-testid="mappings-tab" className="flex flex-col gap-4 px-4 py-3">
      <section className="rounded-md border bg-white p-4 dark:bg-slate-900 dark:border-slate-700">
        <header className="flex items-start justify-between gap-4">
          <div>
            <h2 className="text-sm font-semibold">Metric mapping</h2>
            <p className="mt-1 text-xs text-slate-600 dark:text-slate-300">
              How Trove turns raw harness activity into the cross-harness Tier A metrics on your
              dashboard. Tier B (harness-native) metrics always pass through unchanged — these rows
              only affect <code>trove.harness.*</code>.
            </p>
          </div>
          <button
            type="button"
            data-testid="mappings-reset-all"
            onClick={() => void handleResetAll()}
            disabled={busy}
            className="rounded-md border border-slate-300 bg-white px-2 py-1 text-xs hover:bg-slate-50 disabled:opacity-50 dark:bg-slate-950 dark:border-slate-700 dark:hover:bg-slate-800"
          >
            Reset all to defaults
          </button>
        </header>
      </section>

      {error ? (
        <section
          data-testid="mappings-error"
          className="rounded-md border border-red-300 bg-red-50 p-3 text-xs text-red-800 dark:bg-red-950/40 dark:border-red-700 dark:text-red-200"
        >
          {error.cause.kind === 'internal' ? error.cause.reason : `IPC error: ${error.cause.kind}`}
        </section>
      ) : null}

      <ul className="flex flex-col gap-3">
        {harnesses.map((mapping) => (
          <HarnessCard
            key={mapping.harnessId}
            mapping={mapping}
            busy={busy}
            onToggleEnabled={(next) => void setHarnessEnabled(mapping.harnessId, next)}
          />
        ))}
      </ul>
    </div>
  );
}

interface CardProps {
  mapping: HarnessMapping;
  busy: boolean;
  onToggleEnabled: (next: boolean) => void;
}

const HARNESS_LABELS: Record<HarnessId, string> = {
  'claude-code': 'Claude Code',
  'gemini-cli': 'Gemini CLI',
  'codex-cli': 'OpenAI Codex CLI',
  'qwen-code': 'Qwen Code',
  opencode: 'OpenCode',
  'cursor-ide': 'Cursor IDE',
  'cursor-cli': 'Cursor CLI',
  cline: 'Cline',
  aider: 'Aider',
  'copilot-cli': 'GitHub Copilot CLI',
};

function HarnessCard({ mapping, busy, onToggleEnabled }: CardProps): JSX.Element {
  const synthRows = useMemo(
    () => mapping.sources.filter((s) => s.kind === 'synthesize-from-native'),
    [mapping.sources],
  );
  const hookRows = useMemo(
    () => mapping.sources.filter((s) => s.kind === 'hook-rule'),
    [mapping.sources],
  );

  return (
    <li
      data-testid={`mapping-card-${mapping.harnessId}`}
      className="rounded-md border bg-white p-4 dark:bg-slate-900 dark:border-slate-700"
    >
      <div className="flex items-start justify-between gap-4">
        <div>
          <div className="flex items-center gap-2">
            <h3 className="text-sm font-semibold">{HARNESS_LABELS[mapping.harnessId]}</h3>
            <code className="text-[10px] text-slate-500 dark:text-slate-400">
              {mapping.harnessId}
            </code>
          </div>
          <p className="mt-1 text-xs text-slate-600 dark:text-slate-300">
            {synthRows.length === 0 && hookRows.length === 0
              ? 'No mapping rows shipped yet — emission shape pending upstream verification.'
              : `${synthRows.length} synthesis row${synthRows.length === 1 ? '' : 's'}, ${hookRows.length} hook rule${hookRows.length === 1 ? '' : 's'}.`}
          </p>
        </div>
        <label className="flex items-center gap-1.5 text-xs">
          <input
            type="checkbox"
            checked={mapping.enabled}
            disabled={busy}
            onChange={(e) => onToggleEnabled(e.target.checked)}
            data-testid={`mapping-enabled-${mapping.harnessId}`}
          />
          Enabled
        </label>
      </div>

      {synthRows.length > 0 ? (
        <details className="mt-3">
          <summary className="cursor-pointer text-xs font-medium text-slate-700 dark:text-slate-200">
            Native → Tier A synthesis ({synthRows.length})
          </summary>
          <ul className="mt-2 ml-4 flex flex-col gap-1 text-xs">
            {synthRows.map((row, i) => (
              <li key={`s-${i}`}>
                <SynthesisRow source={row} />
              </li>
            ))}
          </ul>
        </details>
      ) : null}

      {hookRows.length > 0 ? (
        <details className="mt-2">
          <summary className="cursor-pointer text-xs font-medium text-slate-700 dark:text-slate-200">
            Hook / watcher rules ({hookRows.length})
          </summary>
          <ul className="mt-2 ml-4 flex flex-col gap-1 text-xs">
            {hookRows.map((row, i) => (
              <li key={`h-${i}`}>
                <HookRuleRow source={row} />
              </li>
            ))}
          </ul>
        </details>
      ) : null}
    </li>
  );
}

function SynthesisRow({ source }: { source: MappingSource }): JSX.Element | null {
  if (source.kind !== 'synthesize-from-native') return null;
  return (
    <span className="font-mono text-[11px]">
      <code className="rounded bg-slate-100 px-1 py-0.5 dark:bg-slate-800">
        {source.nativeMetric}
      </code>{' '}
      →{' '}
      <code className="rounded bg-emerald-100 px-1 py-0.5 dark:bg-emerald-900/40">
        trove.harness.{tierAMetricSuffix(source.targetMetric)}
      </code>
    </span>
  );
}

function HookRuleRow({ source }: { source: MappingSource }): JSX.Element | null {
  if (source.kind !== 'hook-rule') return null;
  return (
    <span className="font-mono text-[11px]">
      <code className="rounded bg-slate-100 px-1 py-0.5 dark:bg-slate-800">{source.when}</code> →{' '}
      {source.emit === null ? (
        <span className="italic text-slate-500">no emission</span>
      ) : (
        <>
          <code className="rounded bg-emerald-100 px-1 py-0.5 dark:bg-emerald-900/40">
            trove.harness.{tierAMetricSuffix(source.emit.metric)}
          </code>
          {Object.keys(source.emit.attributes).length > 0 ? (
            <>
              {' '}
              <span className="text-slate-500">with</span>{' '}
              <code className="text-slate-700 dark:text-slate-300">
                {Object.entries(source.emit.attributes)
                  .map(([k, v]) => `${k}=${v}`)
                  .join(', ')}
              </code>
            </>
          ) : null}
        </>
      )}
    </span>
  );
}

function tierAMetricSuffix(m: TierAMetric): string {
  return m;
}

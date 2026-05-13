import { useCallback, useMemo, useState } from 'react';

import {
  type AppState,
  type HarnessId,
  type HarnessMapping,
  type MappingSource,
  type TierAMetric,
} from '@trove/shared';

import { TroveIpcError, applyMappings, resetMappingsToDefaults } from '../../lib/ipc.js';
import { Button, Card, CardHeader, CardTitle, Pill, StatusDot } from '../ui/index.js';

interface Props {
  appState: AppState;
  onAppStateRefresh: () => void | Promise<void>;
}

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
    <div data-testid="mappings-tab" className="flex flex-col gap-3 px-4 py-3">
      <Card>
        <CardHeader>
          <div>
            <CardTitle>Metric mapping</CardTitle>
            <p className="mt-1 text-[12px] text-fg-secondary dark:text-fg-secondary-dark">
              How Trove turns raw harness activity into the cross-harness Tier A metrics on your
              dashboard. Tier B (harness-native) metrics always pass through unchanged — these rows
              only affect <code className="font-mono">trove.harness.*</code>.
            </p>
          </div>
          <Button
            variant="secondary"
            size="sm"
            testid="mappings-reset-all"
            onClick={() => void handleResetAll()}
            disabled={busy}
          >
            Reset all to defaults
          </Button>
        </CardHeader>
      </Card>

      {error ? (
        <div
          data-testid="mappings-error"
          className="flex items-start gap-2 rounded-card border border-hairline bg-ios-red/[0.08] px-3 py-2 text-[12px] text-fg-primary dark:border-hairline-dark dark:text-fg-primary-dark"
        >
          <StatusDot status="red" size="md" pulse={false} className="mt-1" />
          <span>
            {error.cause.kind === 'internal'
              ? error.cause.reason
              : `IPC error: ${error.cause.kind}`}
          </span>
        </div>
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

interface CardComponentProps {
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

function HarnessCard({ mapping, busy, onToggleEnabled }: CardComponentProps): JSX.Element {
  const synthRows = useMemo(
    () => mapping.sources.filter((s) => s.kind === 'synthesize-from-native'),
    [mapping.sources],
  );
  const hookRows = useMemo(
    () => mapping.sources.filter((s) => s.kind === 'hook-rule'),
    [mapping.sources],
  );

  return (
    <li>
      <Card as="div" testid={`mapping-card-${mapping.harnessId}`}>
        <div className="flex items-start justify-between gap-4">
          <div>
            <div className="flex items-center gap-2">
              <h3 className="text-[13px] font-semibold tracking-tight text-fg-primary dark:text-fg-primary-dark">
                {HARNESS_LABELS[mapping.harnessId]}
              </h3>
              <code className="text-[10px] text-fg-tertiary dark:text-fg-tertiary-dark">
                {mapping.harnessId}
              </code>
              {mapping.enabled ? (
                <Pill tone="green" size="xs">
                  Enabled
                </Pill>
              ) : (
                <Pill tone="neutral" size="xs">
                  Disabled
                </Pill>
              )}
            </div>
            <p className="mt-1 text-[12px] text-fg-secondary dark:text-fg-secondary-dark">
              {synthRows.length === 0 && hookRows.length === 0
                ? 'No mapping rows shipped yet — emission shape pending upstream verification.'
                : `${synthRows.length} synthesis row${synthRows.length === 1 ? '' : 's'}, ${hookRows.length} hook rule${hookRows.length === 1 ? '' : 's'}.`}
            </p>
          </div>
          <label className="flex items-center gap-1.5 text-[12px] text-fg-secondary dark:text-fg-secondary-dark">
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
            <summary className="cursor-pointer text-[12px] font-medium text-fg-secondary dark:text-fg-secondary-dark">
              Native → Tier A synthesis ({synthRows.length})
            </summary>
            <ul className="ml-4 mt-2 flex flex-col gap-1 text-[12px]">
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
            <summary className="cursor-pointer text-[12px] font-medium text-fg-secondary dark:text-fg-secondary-dark">
              Hook / watcher rules ({hookRows.length})
            </summary>
            <ul className="ml-4 mt-2 flex flex-col gap-1 text-[12px]">
              {hookRows.map((row, i) => (
                <li key={`h-${i}`}>
                  <HookRuleRow source={row} />
                </li>
              ))}
            </ul>
          </details>
        ) : null}
      </Card>
    </li>
  );
}

function SynthesisRow({ source }: { source: MappingSource }): JSX.Element | null {
  if (source.kind !== 'synthesize-from-native') return null;
  return (
    <span className="font-mono text-[11px]">
      <code className="rounded bg-black/[0.06] px-1 py-0.5 dark:bg-white/[0.08]">
        {source.nativeMetric}
      </code>{' '}
      →{' '}
      <code className="rounded bg-ios-green/[0.14] px-1 py-0.5 text-ios-green">
        trove.harness.{tierAMetricSuffix(source.targetMetric)}
      </code>
    </span>
  );
}

function HookRuleRow({ source }: { source: MappingSource }): JSX.Element | null {
  if (source.kind !== 'hook-rule') return null;
  return (
    <span className="font-mono text-[11px]">
      <code className="rounded bg-black/[0.06] px-1 py-0.5 dark:bg-white/[0.08]">
        {source.when}
      </code>{' '}
      →{' '}
      {source.emit === null ? (
        <span className="italic text-fg-tertiary dark:text-fg-tertiary-dark">no emission</span>
      ) : (
        <>
          <code className="rounded bg-ios-green/[0.14] px-1 py-0.5 text-ios-green">
            trove.harness.{tierAMetricSuffix(source.emit.metric)}
          </code>
          {Object.keys(source.emit.attributes).length > 0 ? (
            <>
              {' '}
              <span className="text-fg-tertiary dark:text-fg-tertiary-dark">with</span>{' '}
              <code className="text-fg-secondary dark:text-fg-secondary-dark">
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

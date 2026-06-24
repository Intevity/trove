import { Plus, Search, X } from 'lucide-react';
import { useCallback, useMemo, useState } from 'react';

import { BOTTOM_PINNED_KINDS, PRESETS, type PresetMetadata } from '@trove/collector-presets';
import type {
  AppState,
  Backend,
  BackendHealth,
  BackendHealthStatus,
  BackendInstance,
} from '@trove/shared';

import { useBackendHealth } from '../../hooks/useBackendHealth.js';
import { TroveIpcError, removeBackend, setBackendEnabled } from '../../lib/ipc.js';
import { BackendLogo } from '../../lib/logos.js';
import { Button, Card, CardHeader, CardTitle, Pill, StatusDot } from '../ui/index.js';
import { BackendWizard, type WizardMode } from '../wizard/BackendWizard.js';

interface Props {
  appState: AppState;
  /** Re-fetches `state.json` after a write so the list reflects the
   *  newly-persisted instance. Same callback the Dashboard threads
   *  through to MappingsTab/SettingsTab. */
  onAppStateRefresh: () => void | Promise<void>;
}

type PresetKind = Backend['kind'];
type WizardState = { open: false } | { open: true; mode: WizardMode };

/** Platforms page. Shows every supported destination (one row per
 *  preset kind) with logo + description; configured instances appear
 *  inline under their kind with Edit / Remove affordances. Mirrors the
 *  Harnesses tab's "see them all, search, enable from the list" UX. */
export function PlatformsTab({ appState, onAppStateRefresh }: Props): JSX.Element {
  const [wizardState, setWizardState] = useState<WizardState>({ open: false });
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const healthByBackendId = useBackendHealth();

  const openAddForKind = useCallback((presetKind: PresetKind) => {
    setError(null);
    setWizardState({ open: true, mode: { kind: 'add', presetKind } });
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

  const handleToggleEnabled = useCallback(
    async (id: string, enabled: boolean) => {
      setBusyId(id);
      setError(null);
      try {
        await setBackendEnabled(id, enabled);
        await onAppStateRefresh();
      } catch (e) {
        setError(e instanceof TroveIpcError ? `${e.cause.kind}` : String(e));
      } finally {
        setBusyId(null);
      }
    },
    [onAppStateRefresh],
  );

  // Group configured instances by preset kind so each preset row can
  // render its own instances inline.
  const instancesByKind = useMemo(() => {
    const out: Record<string, BackendInstance[]> = {};
    for (const inst of appState.backends) {
      const k = inst.backend.kind;
      (out[k] ??= []).push(inst);
    }
    return out;
  }, [appState.backends]);

  const q = query.trim().toLowerCase();
  /** Sort order: recommended preset first, then enabled (≥1 configured
   *  instance) destinations alphabetically by label, then disabled
   *  destinations alphabetically, with the generic escape-hatch kinds
   *  pinned at the very bottom regardless of enabled state. */
  const filteredPresets = useMemo(() => {
    const base = q
      ? PRESETS.filter((p) => p.label.toLowerCase().includes(q) || p.kind.includes(q))
      : PRESETS;
    const isEnabled = (p: PresetMetadata): boolean => (instancesByKind[p.kind]?.length ?? 0) > 0;
    return [...base].sort((a, b) => {
      if (a.recommended !== b.recommended) return a.recommended ? -1 : 1;
      const aBottom = BOTTOM_PINNED_KINDS.has(a.kind);
      const bBottom = BOTTOM_PINNED_KINDS.has(b.kind);
      if (aBottom !== bBottom) return aBottom ? 1 : -1;
      if (!aBottom) {
        const aOn = isEnabled(a);
        const bOn = isEnabled(b);
        if (aOn !== bOn) return aOn ? -1 : 1;
      }
      return a.label.localeCompare(b.label);
    });
  }, [q, instancesByKind]);

  if (wizardState.open) {
    return (
      <div className="flex flex-col gap-3 px-4 py-3">
        <BackendWizard mode={wizardState.mode} onComplete={() => void closeWizard()} />
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-3 px-4 py-3">
      {error ? (
        <div
          data-testid="platforms-tab-error"
          className="flex items-start gap-2 rounded-card border border-hairline bg-ios-red/[0.08] px-3 py-2 text-[13px] text-fg-primary dark:border-hairline-dark dark:text-fg-primary-dark"
        >
          <StatusDot status="red" size="md" pulse={false} className="mt-1" />
          <span>{error}</span>
        </div>
      ) : null}

      <Card as="section" padding="sm" testid="platforms-section">
        <CardHeader>
          <CardTitle>Supported platforms</CardTitle>
        </CardHeader>

        <div className="relative mb-2">
          <Search
            size={13}
            aria-hidden="true"
            className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-fg-tertiary dark:text-fg-tertiary-dark"
          />
          <input
            type="text"
            role="searchbox"
            data-testid="platform-search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Filter platforms by name…"
            aria-label="Filter platforms by name"
            className="w-full rounded-[8px] border border-hairline bg-surface-elevated py-1.5 pl-8 pr-8 text-[12px] text-fg-primary placeholder:text-fg-tertiary focus:border-brand focus:outline-none focus:ring-1 focus:ring-brand dark:border-hairline-dark dark:bg-surface-elevated-dark dark:text-fg-primary-dark dark:placeholder:text-fg-tertiary-dark"
          />
          {query ? (
            <button
              type="button"
              data-testid="platform-search-clear"
              aria-label="Clear search"
              onClick={() => setQuery('')}
              className="absolute right-2 top-1/2 -translate-y-1/2 rounded p-0.5 text-fg-tertiary hover:text-fg-primary dark:text-fg-tertiary-dark dark:hover:text-fg-primary-dark"
            >
              <X size={13} aria-hidden="true" />
            </button>
          ) : null}
        </div>

        {filteredPresets.length === 0 ? (
          <p
            className="text-[13px] text-fg-secondary dark:text-fg-secondary-dark"
            data-testid="platforms-no-matches"
          >
            No platforms match “{query.trim()}”.
          </p>
        ) : (
          <ul
            className="divide-y divide-hairline overflow-hidden rounded-card border border-hairline dark:divide-hairline-dark dark:border-hairline-dark"
            data-testid="platforms-list"
          >
            {filteredPresets.map((preset) => (
              <PlatformRow
                key={preset.kind}
                preset={preset}
                instances={instancesByKind[preset.kind] ?? []}
                healthByBackendId={healthByBackendId}
                busyId={busyId}
                onAdd={() => openAddForKind(preset.kind)}
                onEdit={openEdit}
                onRemove={(id) => void handleRemove(id)}
                onToggleEnabled={(id, enabled) => void handleToggleEnabled(id, enabled)}
              />
            ))}
          </ul>
        )}
      </Card>
    </div>
  );
}

interface PlatformRowProps {
  preset: PresetMetadata;
  instances: BackendInstance[];
  healthByBackendId: Map<string, BackendHealth>;
  busyId: string | null;
  onAdd: () => void;
  onEdit: (instance: BackendInstance) => void;
  onRemove: (id: string) => void;
  onToggleEnabled: (id: string, enabled: boolean) => void;
}

function PlatformRow({
  preset,
  instances,
  healthByBackendId,
  busyId,
  onAdd,
  onEdit,
  onRemove,
  onToggleEnabled,
}: PlatformRowProps): JSX.Element {
  const configured = instances.length > 0;
  return (
    <li data-testid={`platform-row-${preset.kind}`} data-configured={configured}>
      <div className="flex items-start gap-3 px-3 py-2.5">
        <BackendLogo kind={preset.kind} size={32} dimmed={!configured} />
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <p className="text-[13px] font-medium text-fg-primary dark:text-fg-primary-dark">
              {preset.label}
            </p>
            {preset.recommended ? (
              <Pill tone="brand" size="xs">
                Recommended
              </Pill>
            ) : null}
            {preset.beta ? (
              <Pill tone="beta" size="xs" testid="preset-beta-badge">
                Beta
              </Pill>
            ) : null}
            {configured ? (
              <span className="flex items-center gap-1">
                <StatusDot status="green" size="sm" />
                <span className="text-[11px] text-fg-secondary dark:text-fg-secondary-dark">
                  {instances.length === 1 ? 'Enabled' : `Enabled · ${instances.length} configured`}
                </span>
              </span>
            ) : null}
          </div>
          <p className="mt-0.5 text-[12px] text-fg-secondary dark:text-fg-secondary-dark">
            {preset.description}
          </p>
        </div>
        <Button
          variant={configured ? 'secondary' : 'primary'}
          size="sm"
          testid={`platform-${preset.kind}-add`}
          onClick={onAdd}
        >
          <Plus size={12} aria-hidden />
          {configured ? 'Add another' : 'Add'}
        </Button>
      </div>

      {configured ? (
        <ul
          className="border-t border-hairline bg-canvas/40 dark:border-hairline-dark dark:bg-canvas-dark/40"
          data-testid={`platform-instances-${preset.kind}`}
        >
          {instances.map((instance) => (
            <BackendInstanceRow
              key={instance.id}
              instance={instance}
              preset={preset}
              health={healthByBackendId.get(instance.id) ?? null}
              busy={busyId === instance.id}
              onEdit={() => onEdit(instance)}
              onRemove={() => onRemove(instance.id)}
              onToggleEnabled={() => onToggleEnabled(instance.id, !instance.enabled)}
            />
          ))}
        </ul>
      ) : null}
    </li>
  );
}

interface BackendInstanceRowProps {
  instance: BackendInstance;
  preset: PresetMetadata;
  health: BackendHealth | null;
  busy: boolean;
  onEdit: () => void;
  onRemove: () => void;
  /** Flip the instance's `enabled` flag. Disabled instances stay in
   *  the list (so the user can re-enable later without re-entering
   *  credentials) but are removed from the collector pipeline and
   *  hidden from the Overview Data flow chart. */
  onToggleEnabled: () => void;
}

/** One configured destination, rendered under its preset's row.
 *  Shows a 4-color status pill driven by the latest `backend-health`
 *  event; click the pill (or anywhere on the label area) to inline-
 *  expand the latest error / last-success detail. Edit, Disable/Enable,
 *  and Remove buttons stop click propagation so they don't toggle the
 *  panel. When the instance is disabled the row dims and the health
 *  pill collapses to a neutral "disabled" marker — no scrape data is
 *  meaningful because the collector isn't forwarding. */
function BackendInstanceRow({
  instance,
  preset,
  health,
  busy,
  onEdit,
  onRemove,
  onToggleEnabled,
}: BackendInstanceRowProps): JSX.Element {
  const [expanded, setExpanded] = useState(false);
  const enabled = instance.enabled;
  const status: BackendHealthStatus = enabled ? (health?.status ?? 'gray') : 'gray';
  const dotColor: 'green' | 'amber' | 'red' | 'gray' = status;
  return (
    <li
      data-testid={`platform-instance-${instance.id}`}
      data-health-status={status}
      data-enabled={enabled}
      className="border-b border-hairline last:border-b-0 dark:border-hairline-dark"
    >
      <div className={`flex items-center gap-2 px-3 py-1.5 pl-12 ${enabled ? '' : 'opacity-55'}`}>
        <button
          type="button"
          aria-expanded={expanded}
          aria-label={`Toggle health detail for ${instance.label ?? preset.label}`}
          data-testid={`platform-instance-toggle-${instance.id}`}
          onClick={() => setExpanded((v) => !v)}
          className="flex min-w-0 flex-1 items-center gap-2 truncate rounded text-left text-[12px] text-fg-primary hover:bg-surface-elevated/60 focus:outline-none focus:ring-1 focus:ring-brand dark:text-fg-primary-dark dark:hover:bg-surface-elevated-dark/60"
        >
          <StatusDot
            status={dotColor}
            size="sm"
            pulse={enabled && status === 'green'}
            label={enabled ? `${preset.label} health: ${status}` : `${preset.label}: disabled`}
          />
          <span className="min-w-0 flex-1 truncate">
            {instance.label ?? preset.label}
            {enabled ? null : (
              <span className="text-fg-tertiary dark:text-fg-tertiary-dark"> · disabled</span>
            )}
            <BackendDetail backend={instance.backend} />
          </span>
        </button>
        <Button
          variant="ghost"
          size="sm"
          testid={`platform-instance-edit-${instance.id}`}
          onClick={onEdit}
          disabled={busy}
        >
          Edit
        </Button>
        <Button
          variant="ghost"
          size="sm"
          testid={`platform-instance-toggle-enabled-${instance.id}`}
          onClick={onToggleEnabled}
          disabled={busy}
        >
          {enabled ? 'Disable' : 'Enable'}
        </Button>
        <Button
          variant="ghost"
          size="sm"
          testid={`platform-instance-remove-${instance.id}`}
          onClick={onRemove}
          disabled={busy}
        >
          {busy ? '…' : 'Remove'}
        </Button>
      </div>
      {expanded ? <BackendHealthDetail health={health} /> : null}
    </li>
  );
}

/** Inline-expand panel below a destination row. Renders the most-recent
 *  exporter error text, the count of failed batches in the rolling
 *  window, and the relative time since the last successful send.
 *  No data ⇒ shows a placeholder rather than an empty panel. */
function BackendHealthDetail({ health }: { health: BackendHealth | null }): JSX.Element {
  if (!health) {
    return (
      <div
        data-testid="platform-instance-health-empty"
        className="px-3 pb-2 pl-12 text-[11px] text-fg-tertiary dark:text-fg-tertiary-dark"
      >
        No scrape data yet — health pill will update within ~5 s once the bundled collector finishes
        a scrape tick.
      </div>
    );
  }
  const lastSuccess = relativeTime(health.lastSuccessAt);
  const lastError = relativeTime(health.lastErrorAt);
  return (
    <div
      data-testid="platform-instance-health-detail"
      className="space-y-1 px-3 pb-2 pl-12 text-[11px] text-fg-secondary dark:text-fg-secondary-dark"
    >
      <div className="flex gap-3">
        <span>
          <Pill tone={statusToTone(health.status)} size="xs">
            {health.status}
          </Pill>
        </span>
        <span data-testid="platform-instance-health-window-sent">
          Sent in last 60 s: <strong>{health.windowSent}</strong>
        </span>
        <span data-testid="platform-instance-health-window-failed">
          Failed in last 60 s: <strong>{health.windowFailed}</strong>
        </span>
      </div>
      <div className="flex gap-3">
        <span>Last successful send: {lastSuccess ?? '—'}</span>
        <span>Last error: {lastError ?? '—'}</span>
      </div>
      {health.lastErrorMsg ? (
        <div
          data-testid="platform-instance-health-error-msg"
          className="overflow-hidden text-ellipsis whitespace-nowrap font-mono text-fg-primary dark:text-fg-primary-dark"
          title={health.lastErrorMsg}
        >
          {health.lastErrorMsg}
        </div>
      ) : null}
    </div>
  );
}

function statusToTone(status: BackendHealthStatus): 'neutral' | 'green' | 'amber' | 'red' {
  switch (status) {
    case 'green':
      return 'green';
    case 'amber':
      return 'amber';
    case 'red':
      return 'red';
    case 'gray':
    default:
      return 'neutral';
  }
}

function relativeTime(iso: string | null | undefined): string | null {
  if (!iso) return null;
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return null;
  const ms = Date.now() - t;
  if (ms < 0) return 'just now';
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s}s ago`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.round(m / 60);
  return `${h}h ago`;
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
    case 'new-relic':
      return (
        <span className="text-fg-tertiary dark:text-fg-tertiary-dark">
          {' '}
          · {backend.region.toUpperCase()}
        </span>
      );
    case 'splunk-observability':
      return (
        <span className="text-fg-tertiary dark:text-fg-tertiary-dark">
          {' '}
          · realm {backend.realm}
        </span>
      );
    case 'dynatrace':
      return (
        <span className="text-fg-tertiary dark:text-fg-tertiary-dark">
          {' '}
          · {backend.environmentUrl}
        </span>
      );
    case 'elastic':
    case 'opensearch':
    case 'clickstack':
    case 'sentry':
      return (
        <span className="text-fg-tertiary dark:text-fg-tertiary-dark"> · {backend.endpoint}</span>
      );
    case 'openobserve':
      return (
        <span className="text-fg-tertiary dark:text-fg-tertiary-dark">
          {' '}
          · {backend.endpoint} · org {backend.organization}
        </span>
      );
    case 'chronosphere':
      return (
        <span className="text-fg-tertiary dark:text-fg-tertiary-dark">
          {' '}
          · tenant {backend.tenant}
        </span>
      );
  }
}

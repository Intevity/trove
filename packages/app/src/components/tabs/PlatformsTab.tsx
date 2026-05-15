import { Plus, Search, X } from 'lucide-react';
import { useCallback, useMemo, useState } from 'react';

import { PRESETS, type PresetMetadata } from '@trove/collector-presets';
import type { AppState, Backend, BackendInstance } from '@trove/shared';

import { TroveIpcError, removeBackend } from '../../lib/ipc.js';
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
  const filteredPresets = useMemo(
    () =>
      q ? PRESETS.filter((p) => p.label.toLowerCase().includes(q) || p.kind.includes(q)) : PRESETS,
    [q],
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
                busyId={busyId}
                onAdd={() => openAddForKind(preset.kind)}
                onEdit={openEdit}
                onRemove={(id) => void handleRemove(id)}
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
  busyId: string | null;
  onAdd: () => void;
  onEdit: (instance: BackendInstance) => void;
  onRemove: (id: string) => void;
}

function PlatformRow({
  preset,
  instances,
  busyId,
  onAdd,
  onEdit,
  onRemove,
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
            <li
              key={instance.id}
              data-testid={`platform-instance-${instance.id}`}
              className="flex items-center gap-2 px-3 py-1.5 pl-12"
            >
              <span className="min-w-0 flex-1 truncate text-[12px] text-fg-primary dark:text-fg-primary-dark">
                {instance.label ?? preset.label}
                <BackendDetail backend={instance.backend} />
              </span>
              <Button
                variant="ghost"
                size="sm"
                testid={`platform-instance-edit-${instance.id}`}
                onClick={() => onEdit(instance)}
                disabled={busyId === instance.id}
              >
                Edit
              </Button>
              <Button
                variant="ghost"
                size="sm"
                testid={`platform-instance-remove-${instance.id}`}
                onClick={() => onRemove(instance.id)}
                disabled={busyId === instance.id}
              >
                {busyId === instance.id ? 'Removing…' : 'Remove'}
              </Button>
            </li>
          ))}
        </ul>
      ) : null}
    </li>
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

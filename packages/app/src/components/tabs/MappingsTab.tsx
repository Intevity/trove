import { ChevronDown, ChevronRight, Search, Workflow, X } from 'lucide-react';
import { useCallback, useEffect, useMemo, useRef, useState, type JSX } from 'react';

import {
  type AppState,
  type DetectedHarness,
  type HarnessId,
  type HarnessMapping,
  type MappingState,
  type TroveMetricDefinition,
} from '@trove/shared';

import { TroveIpcError, applyMappings, resetMappingsToDefaults } from '../../lib/ipc.js';
import { Button, Card, CardHeader, CardTitle, Pill } from '../ui/index.js';
import { HarnessMappingEditor } from './mappings/HarnessMappingEditor.js';
import { MappingDiffSheet } from './mappings/MappingDiffSheet.js';
import { MappingPreviewSheet } from './mappings/MappingPreviewSheet.js';
import { MetricsCatalog } from './mappings/MetricsCatalog.js';
import { mappingStatesEqual, validateCatalog } from './mappings/validate.js';

interface Props {
  appState: AppState;
  /** Latest detection sweep from `useDetectedHarnesses`, threaded through
   *  the Dashboard. When provided, the tab groups undetected harnesses
   *  into a collapsible section at the bottom (mirroring the Harnesses
   *  page's installed-vs-not split). When undefined or empty (e.g. tests
   *  that don't mock detection), every harness renders in the main list. */
  detectedHarnesses?: DetectedHarness[];
  onAppStateRefresh: () => void | Promise<void>;
}

interface PreviewTarget {
  harnessId: HarnessId;
  sourceIndex: number;
}

/** Top-level Mappings tab. Owns the draft state for the catalog and
 *  each per-harness mapping. Edits are local until the user clicks
 *  Apply on a specific harness; clicking "Apply all" applies every
 *  dirty section in one IPC call. */
export function MappingsTab({
  appState,
  detectedHarnesses,
  onAppStateRefresh,
}: Props): JSX.Element {
  const persisted = appState.mappings;

  // The draft state mirrors the persisted shape exactly. Children edit
  // pieces of this draft through narrowed callbacks. When a section's
  // draft slice differs from the persisted slice we surface a Modified
  // pill; Apply pushes the whole draft to the Rust side.
  const [draft, setDraft] = useState<MappingState>(persisted);
  const [error, setError] = useState<TroveIpcError | null>(null);
  const [busy, setBusy] = useState(false);
  const [previewTarget, setPreviewTarget] = useState<PreviewTarget | null>(null);
  const [diffOpen, setDiffOpen] = useState<HarnessId | 'all' | null>(null);
  const [harnessFilter, setHarnessFilter] = useState('');
  const [showNotDetected, setShowNotDetected] = useState(false);

  // When the upstream persisted state changes (after a successful
  // apply, or after another window mutated state), re-seed the draft
  // for any harness that wasn't dirty. We don't auto-overwrite dirty
  // drafts — the user's in-flight edits survive a refresh.
  // (Implementation: a useEffect would be the natural place, but a
  // simple re-seed on mount + after-apply suffices for v1 since
  // appState refreshes happen via explicit IPC reload.)

  const catalog = draft.metrics;
  const catalogDirty = useMemo(
    () => !arrayShallowEqual(persisted.metrics, draft.metrics),
    [persisted.metrics, draft.metrics],
  );
  const catalogIssues = validateCatalog(catalog);

  const overallDirty = !mappingStatesEqual(persisted, draft);

  const updateCatalog = useCallback((next: TroveMetricDefinition[]) => {
    setDraft((prev) => ({ ...prev, metrics: next }));
  }, []);

  const updateHarness = useCallback((id: HarnessId, next: HarnessMapping) => {
    setDraft((prev) => ({
      ...prev,
      harnesses: prev.harnesses.map((h) => (h.harnessId === id ? next : h)),
    }));
  }, []);

  const discardHarness = useCallback(
    (id: HarnessId) => {
      const upstream = persisted.harnesses.find((h) => h.harnessId === id);
      if (!upstream) return;
      updateHarness(id, upstream);
    },
    [persisted.harnesses, updateHarness],
  );

  const discardAll = useCallback(() => {
    setDraft(persisted);
  }, [persisted]);

  const applyAll = useCallback(async () => {
    if (catalogIssues.some((i) => i.severity === 'error')) {
      setError(
        new TroveIpcError({ kind: 'internal', reason: 'Fix catalog errors before applying' }),
      );
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await applyMappings(draft);
      await onAppStateRefresh();
    } catch (err) {
      if (err instanceof TroveIpcError) setError(err);
      else setError(new TroveIpcError({ kind: 'internal', reason: String(err) }));
    } finally {
      setBusy(false);
    }
  }, [catalogIssues, draft, onAppStateRefresh]);

  const applyHarness = useCallback(
    async (id: HarnessId) => {
      // Apply requires the full state; we send the current draft (which
      // includes any other dirty harnesses too). To keep apply atomic
      // per harness, we'd need a different IPC; the current design
      // bundles the whole state.
      void id;
      await applyAll();
    },
    [applyAll],
  );

  const handleResetAll = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      await resetMappingsToDefaults();
      await onAppStateRefresh();
      // The refreshed appState propagates via props; reset the local
      // draft so it doesn't lag behind the new persisted state.
      // Caller's onAppStateRefresh re-renders this component with the
      // new appState prop; we reset draft to whatever lands there.
    } catch (err) {
      if (err instanceof TroveIpcError) setError(err);
    } finally {
      setBusy(false);
    }
  }, [onAppStateRefresh]);

  // Re-sync the draft when the persisted state from props changes
  // (e.g. after a successful applyAll caused the parent to re-fetch).
  // Sections that were clean adopt the new persisted slice; dirty
  // sections keep their in-flight edits until the user discards or
  // re-applies. The `lastPersistedRef` keeps the *previous* persisted
  // snapshot so we can tell which sections were dirty before the prop
  // change.
  const lastPersistedRef = useRef<MappingState>(persisted);
  useEffect(() => {
    const prev = lastPersistedRef.current;
    if (prev === persisted) return;
    setDraft((currentDraft) => {
      // Catalog: adopt new persisted catalog only if the draft still
      // matches the prior persisted catalog.
      const nextMetrics = arrayShallowEqual(currentDraft.metrics, prev.metrics)
        ? persisted.metrics
        : currentDraft.metrics;
      // Harnesses: per-harness re-sync.
      const nextHarnesses = persisted.harnesses.map((newUpstream) => {
        const draftSlice = currentDraft.harnesses.find(
          (h) => h.harnessId === newUpstream.harnessId,
        );
        const priorUpstream = prev.harnesses.find((h) => h.harnessId === newUpstream.harnessId);
        // No prior or no draft slice → adopt upstream.
        if (!draftSlice || !priorUpstream) return newUpstream;
        // Was clean → adopt upstream. Else keep draft.
        return shallowMappingEqual(draftSlice, priorUpstream) ? newUpstream : draftSlice;
      });
      return {
        schemaVersion: persisted.schemaVersion,
        metrics: nextMetrics,
        harnesses: nextHarnesses,
      };
    });
    lastPersistedRef.current = persisted;
  }, [persisted]);

  // Filter the harness cards by a free-text match against the harness
  // id. Done case-insensitively so users can type "antigravity" and find
  // `antigravity-cli`. We keep the filter local to this tab — it doesn't
  // touch persisted state and resets when the tab unmounts.
  const filteredHarnesses = useMemo(() => {
    const q = harnessFilter.trim().toLowerCase();
    if (!q) return draft.harnesses;
    return draft.harnesses.filter((h) => h.harnessId.toLowerCase().includes(q));
  }, [draft.harnesses, harnessFilter]);

  // Map of harnessId → detected boolean. When no detection data is
  // available (e.g. tests that don't mock list_detected_harnesses), we
  // fall back to an empty map and treat every harness as detected so
  // nothing gets hidden.
  const detectedById = useMemo<Map<HarnessId, boolean>>(() => {
    const m = new Map<HarnessId, boolean>();
    for (const d of detectedHarnesses ?? []) {
      m.set(d.id, d.detected);
    }
    return m;
  }, [detectedHarnesses]);
  // Detection-only harnesses (no config-patching adapter — e.g. Sentinel,
  // Junie, Devin) can't be auto-wired by Trove; their telemetry is forwarded
  // in by the tool itself. We always surface them so the user can customize
  // their mappings ahead of (or without) on-disk detection.
  const adapterById = useMemo<Map<HarnessId, boolean>>(() => {
    const m = new Map<HarnessId, boolean>();
    for (const d of detectedHarnesses ?? []) {
      m.set(d.id, d.adapterAvailable);
    }
    return m;
  }, [detectedHarnesses]);
  const hasDetectionData = (detectedHarnesses?.length ?? 0) > 0;

  const { detectedList, notDetectedList } = useMemo(() => {
    const detected: HarnessMapping[] = [];
    const notDetected: HarnessMapping[] = [];
    for (const h of filteredHarnesses) {
      const isDetected = detectedById.get(h.harnessId) === true;
      const detectionOnly = adapterById.get(h.harnessId) === false;
      if (!hasDetectionData || isDetected || detectionOnly) {
        detected.push(h);
      } else {
        notDetected.push(h);
      }
    }
    return { detectedList: detected, notDetectedList: notDetected };
  }, [filteredHarnesses, hasDetectionData, detectedById, adapterById]);

  // Shared row renderer so detected and not-detected lists both pull
  // from the same draft/persist plumbing. Returns null when no upstream
  // persisted entry exists (defensive — every draft harness mirrors a
  // persisted one).
  const renderHarnessLi = (draftMapping: HarnessMapping): JSX.Element | null => {
    const upstream = persisted.harnesses.find((h) => h.harnessId === draftMapping.harnessId);
    if (!upstream) return null;
    const dirty = !shallowMappingEqual(upstream, draftMapping);
    return (
      <li key={draftMapping.harnessId}>
        <HarnessMappingEditor
          persisted={upstream}
          draft={draftMapping}
          dirty={dirty}
          catalog={catalog}
          busy={busy}
          onDraftChange={(next) => updateHarness(draftMapping.harnessId, next)}
          onApply={() => applyHarness(draftMapping.harnessId)}
          onDiscard={() => discardHarness(draftMapping.harnessId)}
          onPreviewRule={(sourceIndex) =>
            setPreviewTarget({ harnessId: draftMapping.harnessId, sourceIndex })
          }
          onShowDiff={() => setDiffOpen(draftMapping.harnessId)}
        />
      </li>
    );
  };

  return (
    <div className="flex flex-col gap-3 px-4 py-3">
      <Card>
        <CardHeader className="flex-col items-stretch gap-0">
          <div className="flex flex-nowrap items-center justify-between gap-x-6">
            <CardTitle className="flex items-center gap-1.5 whitespace-nowrap">
              <Workflow size={14} strokeWidth={2.4} className="text-brand" aria-hidden="true" />
              Mapping rules
            </CardTitle>
            <div className="flex flex-nowrap items-center gap-2">
              <Button
                size="sm"
                variant="secondary"
                onClick={() => setDiffOpen('all')}
                disabled={!overallDirty}
              >
                View full diff
              </Button>
              <Button
                size="sm"
                variant="secondary"
                onClick={discardAll}
                disabled={!overallDirty || busy}
              >
                Discard all
              </Button>
              <Button
                size="sm"
                variant="primary"
                onClick={applyAll}
                disabled={!overallDirty || busy}
                loading={busy}
              >
                Apply all
              </Button>
              <Button
                size="sm"
                variant="secondary"
                onClick={handleResetAll}
                disabled={busy}
                testid="reset-mappings"
              >
                Reset to defaults
              </Button>
            </div>
          </div>
          <p className="mt-2 text-[12px] text-fg-tertiary dark:text-fg-tertiary-dark">
            Configure how each harness&apos;s raw signals translate into the cross-harness Trove
            metrics. Tier B (harness-native) metrics always pass through unchanged.
          </p>
        </CardHeader>
      </Card>

      {error ? (
        <Card>
          <p className="text-[12px] text-ios-red">
            {error.cause.kind === 'internal' ? error.cause.reason : error.cause.kind}
          </p>
        </Card>
      ) : null}

      <MetricsCatalog catalog={catalog} dirty={catalogDirty} onChange={updateCatalog} />

      <div className="relative">
        <Search
          className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-fg-tertiary dark:text-fg-tertiary-dark"
          aria-hidden
        />
        <input
          type="search"
          value={harnessFilter}
          onChange={(e) => setHarnessFilter(e.target.value)}
          placeholder="Filter harnesses…"
          aria-label="Filter harnesses"
          className="w-full rounded-[8px] border border-hairline bg-surface-elevated py-1.5 pl-8 pr-8 text-[12px] text-fg-primary placeholder:text-fg-tertiary focus:border-brand focus:outline-none focus:ring-1 focus:ring-brand dark:border-hairline-dark dark:bg-surface-elevated-dark dark:text-fg-primary-dark dark:placeholder:text-fg-tertiary-dark"
        />
        {harnessFilter ? (
          <button
            type="button"
            onClick={() => setHarnessFilter('')}
            aria-label="Clear filter"
            className="absolute right-2 top-1/2 -translate-y-1/2 rounded p-0.5 text-fg-tertiary hover:bg-black/[0.06] hover:text-fg-primary dark:hover:bg-white/[0.08] dark:hover:text-fg-primary-dark"
          >
            <X className="h-3.5 w-3.5" />
          </button>
        ) : null}
      </div>

      {filteredHarnesses.length === 0 ? (
        <Card>
          <p className="text-[12px] text-fg-tertiary dark:text-fg-tertiary-dark">
            No harnesses match {JSON.stringify(harnessFilter)}.
          </p>
        </Card>
      ) : (
        <>
          {detectedList.length > 0 ? (
            <ul className="flex flex-col gap-3">
              {detectedList.map((draftMapping) => renderHarnessLi(draftMapping))}
            </ul>
          ) : hasDetectionData ? (
            <Card>
              <p className="text-[12px] text-fg-tertiary dark:text-fg-tertiary-dark">
                None of the harnesses matching {JSON.stringify(harnessFilter || '')} are detected on
                this system. Expand &quot;Not detected&quot; below to see the rest.
              </p>
            </Card>
          ) : null}

          {notDetectedList.length > 0 ? (
            <div className="rounded-card border border-hairline dark:border-hairline-dark">
              <button
                type="button"
                onClick={() => setShowNotDetected((v) => !v)}
                aria-expanded={showNotDetected}
                className="flex w-full flex-nowrap items-center gap-2 px-3 py-2 text-left text-[12px] text-fg-secondary hover:bg-black/[0.03] dark:text-fg-secondary-dark dark:hover:bg-white/[0.04]"
              >
                {showNotDetected ? (
                  <ChevronDown className="h-3.5 w-3.5 shrink-0" aria-hidden />
                ) : (
                  <ChevronRight className="h-3.5 w-3.5 shrink-0" aria-hidden />
                )}
                <span className="whitespace-nowrap font-medium">Not detected</span>
                <Pill tone="brand">{notDetectedList.length}</Pill>
              </button>
              {showNotDetected ? (
                <ul className="flex flex-col gap-3 border-t border-hairline px-3 py-3 dark:border-hairline-dark">
                  {notDetectedList.map((draftMapping) => renderHarnessLi(draftMapping))}
                </ul>
              ) : null}
            </div>
          ) : null}
        </>
      )}

      {previewTarget ? (
        <MappingPreviewSheet
          open
          onClose={() => setPreviewTarget(null)}
          draftState={draft}
          harnessId={previewTarget.harnessId}
          sourceIndex={previewTarget.sourceIndex}
        />
      ) : null}

      {diffOpen ? (
        <MappingDiffSheet
          open
          onClose={() => setDiffOpen(null)}
          persisted={diffOpen === 'all' ? persisted : narrowToHarness(persisted, diffOpen)}
          draft={diffOpen === 'all' ? draft : narrowToHarness(draft, diffOpen)}
        />
      ) : null}
    </div>
  );
}

function shallowMappingEqual(a: HarnessMapping, b: HarnessMapping): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

function arrayShallowEqual<T>(a: T[], b: T[]): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

function narrowToHarness(state: MappingState, id: HarnessId): MappingState {
  return {
    ...state,
    harnesses: state.harnesses.filter((h) => h.harnessId === id),
  };
}

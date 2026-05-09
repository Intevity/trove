import { diffLines } from 'diff';
import { useState } from 'react';

import type {
  ApplyOptions,
  ConflictAction,
  ConflictPayload,
  ConflictResolutionOutcome,
  HarnessId,
  SiblingPaths,
} from '@trove/shared';

import { TroveIpcError, resolveConflict } from '../lib/ipc.js';

export interface ConflictResolverProps {
  harnessId: HarnessId;
  conflict: ConflictPayload;
  options: ApplyOptions;
  /** Fires after a successful resolution. The parent decides what to do
   *  next (close modal, refresh state, etc.). */
  onResolved: (outcome: ConflictResolutionOutcome) => void;
  /** Cancel the resolver — back out without writing anything. */
  onCancel: () => void;
}

type Mode = 'three-way' | 'two-way';

/** Sprint 8 — 3-way merge resolver. Surfaces when `apply_patch` returns
 *  `region-conflict-detected`. Renders three panes (Original / Yours /
 *  Trove's) and offers three explicit actions; orphan-block payloads
 *  (no prior baseline in `state.json`) collapse to a 2-pane fallback. */
export function ConflictResolver({
  harnessId,
  conflict,
  options,
  onResolved,
  onCancel,
}: ConflictResolverProps): JSX.Element {
  const [pending, setPending] = useState<ConflictAction['kind'] | null>(null);
  const [actionError, setActionError] = useState<TroveIpcError | null>(null);
  const [siblingPaths, setSiblingPaths] = useState<SiblingPaths | null>(null);

  const mode: Mode = conflict.originalRegionPayload === null ? 'two-way' : 'three-way';

  const dispatch = async (action: ConflictAction): Promise<void> => {
    setPending(action.kind);
    setActionError(null);
    try {
      const outcome = await resolveConflict(harnessId, action);
      if (outcome.status === 'merge-deferred') {
        setSiblingPaths(outcome.siblingPaths);
      }
      onResolved(outcome);
    } catch (err) {
      setActionError(err instanceof TroveIpcError ? err : null);
    } finally {
      setPending(null);
    }
  };

  if (siblingPaths) {
    return (
      <SiblingPathsBanner
        siblingPaths={siblingPaths}
        onCancel={onCancel}
        data-testid="conflict-resolver"
      />
    );
  }

  return (
    <div
      className="space-y-4 px-5 py-4"
      data-testid="conflict-resolver"
      data-mode={mode}
      role="region"
      aria-label="3-way merge resolver"
    >
      <p className="rounded-md border border-amber-300 bg-amber-50 px-3 py-2 text-sm text-amber-900 dark:border-amber-700 dark:bg-amber-950 dark:text-amber-100">
        A managed Trove block in <code className="font-mono text-xs">{conflict.configPath}</code>{' '}
        was edited outside Trove. Pick how to resolve — Trove will not silently overwrite your
        changes.
      </p>

      {mode === 'three-way' ? (
        <Pane
          title="Original — what Trove last wrote"
          payload={conflict.originalRegionPayload ?? ''}
          mutedTone
          testId="conflict-pane-original"
        />
      ) : (
        <p
          className="rounded-md border border-slate-300 bg-slate-50 px-3 py-2 text-xs text-slate-700 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-300"
          data-testid="conflict-orphan-notice"
        >
          No prior baseline is on file for this harness — Trove can&apos;t show what it last wrote.
          Compare your version against Trove&apos;s intended write below.
        </p>
      )}

      <DiffPane
        title="Yours — what's in the file now"
        baseline={conflict.originalRegionPayload}
        payload={conflict.currentRegionPayload}
        testId="conflict-pane-yours"
      />

      <DiffPane
        title="Trove's — what apply would write"
        baseline={conflict.originalRegionPayload}
        payload={conflict.theirsRegionPayload}
        testId="conflict-pane-theirs"
      />

      {actionError ? (
        <div
          className="rounded-md border border-red-300 bg-red-50 px-3 py-2 text-sm text-red-900 dark:border-red-700 dark:bg-red-950 dark:text-red-200"
          data-testid="conflict-action-error"
        >
          <p className="font-medium">Resolution failed</p>
          <p className="mt-1 text-xs">{actionError.cause.kind}</p>
        </div>
      ) : null}

      <div className="flex flex-wrap items-center justify-end gap-2 border-t border-slate-200 pt-3 dark:border-slate-800">
        <button
          type="button"
          onClick={onCancel}
          className="rounded-md border border-slate-300 bg-white px-3 py-1.5 text-sm text-slate-900 transition hover:bg-slate-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-100 dark:hover:bg-slate-800"
          data-testid="conflict-cancel"
        >
          Cancel
        </button>
        <button
          type="button"
          onClick={() => void dispatch({ kind: 'merge-manually', options })}
          disabled={pending !== null}
          className="rounded-md border border-slate-300 bg-white px-3 py-1.5 text-sm text-slate-900 transition hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-100 dark:hover:bg-slate-800"
          data-testid="conflict-merge-manually"
        >
          {pending === 'merge-manually' ? 'Writing siblings…' : 'Merge manually'}
        </button>
        <button
          type="button"
          onClick={() => void dispatch({ kind: 'keep-mine' })}
          disabled={pending !== null}
          className="rounded-md border border-slate-300 bg-white px-3 py-1.5 text-sm text-slate-900 transition hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-100 dark:hover:bg-slate-800"
          data-testid="conflict-keep-mine"
        >
          {pending === 'keep-mine' ? 'Saving baseline…' : 'Keep mine'}
        </button>
        <button
          type="button"
          onClick={() => void dispatch({ kind: 'take-theirs', options })}
          disabled={pending !== null}
          className="rounded-md bg-slate-900 px-3 py-1.5 text-sm font-medium text-white transition hover:bg-slate-700 disabled:cursor-not-allowed disabled:opacity-50 dark:bg-slate-100 dark:text-slate-900 dark:hover:bg-slate-200"
          data-testid="conflict-take-theirs"
        >
          {pending === 'take-theirs' ? 'Overwriting…' : "Take Trove's"}
        </button>
      </div>
    </div>
  );
}

interface PaneProps {
  title: string;
  payload: string;
  mutedTone?: boolean;
  testId: string;
}

/** Read-only payload pane, used for "Original" in 3-way mode. */
function Pane({ title, payload, mutedTone, testId }: PaneProps): JSX.Element {
  return (
    <section data-testid={testId}>
      <h3 className="mb-1 text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-400">
        {title}
      </h3>
      <pre
        className={
          mutedTone
            ? 'max-h-[20vh] overflow-auto rounded-md border border-slate-200 bg-slate-50 p-3 text-xs text-slate-600 dark:border-slate-800 dark:bg-slate-950 dark:text-slate-400'
            : 'max-h-[20vh] overflow-auto rounded-md border border-slate-200 bg-slate-50 p-3 text-xs leading-snug dark:border-slate-800 dark:bg-slate-950'
        }
      >
        {payload || <span className="text-slate-400 italic">(empty)</span>}
      </pre>
    </section>
  );
}

interface DiffPaneProps {
  title: string;
  /** Original payload to diff against. `null` falls back to a plain
   *  payload render with no diff overlay. */
  baseline: string | null;
  payload: string;
  testId: string;
}

/** Payload pane with a `diffLines` overlay against `baseline`. Renders
 *  added/removed/unchanged segments with colour cues so the user can
 *  see at a glance what's different. */
function DiffPane({ title, baseline, payload, testId }: DiffPaneProps): JSX.Element {
  if (baseline === null) {
    return <Pane title={title} payload={payload} testId={testId} />;
  }
  const parts = diffLines(baseline, payload);
  return (
    <section data-testid={testId}>
      <h3 className="mb-1 text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-400">
        {title}
      </h3>
      <pre className="max-h-[24vh] overflow-auto rounded-md border border-slate-200 bg-slate-50 p-3 text-xs leading-snug dark:border-slate-800 dark:bg-slate-950">
        {parts.map((part, idx) => (
          <span
            key={idx}
            className={
              part.added
                ? 'block bg-emerald-100 text-emerald-900 dark:bg-emerald-950 dark:text-emerald-200'
                : part.removed
                  ? 'block bg-red-100 text-red-900 dark:bg-red-950 dark:text-red-200'
                  : 'block text-slate-600 dark:text-slate-400'
            }
          >
            {(part.added ? '+ ' : part.removed ? '- ' : '  ') + part.value.replace(/\n$/, '')}
          </span>
        ))}
      </pre>
    </section>
  );
}

interface SiblingPathsBannerProps {
  siblingPaths: SiblingPaths;
  onCancel: () => void;
  'data-testid'?: string;
}

/** Banner shown after a successful `merge-manually` action. Surfaces
 *  the sibling-file paths and the host-config path so the user can
 *  open them in their editor. */
function SiblingPathsBanner({
  siblingPaths,
  onCancel,
  'data-testid': testId,
}: SiblingPathsBannerProps): JSX.Element {
  return (
    <div className="space-y-3 px-5 py-4" data-testid={testId} data-state="merge-deferred">
      <p className="rounded-md border border-emerald-300 bg-emerald-50 px-3 py-2 text-sm text-emerald-900 dark:border-emerald-700 dark:bg-emerald-950 dark:text-emerald-100">
        Sibling files written next to your config. Open the host file in your editor and merge by
        hand. Re-apply once you&apos;re done — Trove will hash the result and either accept it (no
        conflict) or surface this resolver again.
      </p>
      <dl className="space-y-1 text-xs font-mono">
        <PathLine label="Host config" value={siblingPaths.host} testId="conflict-sibling-host" />
        <PathLine
          label="Trove's intended"
          value={siblingPaths.theirs}
          testId="conflict-sibling-theirs"
        />
        <PathLine
          label="Original baseline"
          value={siblingPaths.original}
          testId="conflict-sibling-original"
        />
      </dl>
      <div className="flex justify-end pt-2">
        <button
          type="button"
          onClick={onCancel}
          className="rounded-md border border-slate-300 bg-white px-3 py-1.5 text-sm text-slate-900 transition hover:bg-slate-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-100 dark:hover:bg-slate-800"
          data-testid="conflict-merge-deferred-close"
        >
          Close
        </button>
      </div>
    </div>
  );
}

interface PathLineProps {
  label: string;
  value: string;
  testId: string;
}

function PathLine({ label, value, testId }: PathLineProps): JSX.Element {
  return (
    <div className="flex flex-col">
      <dt className="text-[10px] uppercase tracking-wide text-slate-500 dark:text-slate-400">
        {label}
      </dt>
      <dd className="break-all text-slate-900 dark:text-slate-100" data-testid={testId}>
        {value}
      </dd>
    </div>
  );
}

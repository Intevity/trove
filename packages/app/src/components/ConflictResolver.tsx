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
import { Button, StatusDot } from './ui/index.js';

export interface ConflictResolverProps {
  harnessId: HarnessId;
  conflict: ConflictPayload;
  options: ApplyOptions;
  onResolved: (outcome: ConflictResolutionOutcome) => void;
  onCancel: () => void;
}

type Mode = 'three-way' | 'two-way';

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
      <div className="flex items-start gap-2 rounded-card border border-hairline bg-ios-orange/[0.08] px-3 py-2 text-[13px] text-fg-primary dark:border-hairline-dark dark:text-fg-primary-dark">
        <StatusDot status="amber" size="md" pulse={false} className="mt-1" />
        <p>
          A managed Trove block in{' '}
          <code className="font-mono text-[11px]">{conflict.configPath}</code> was edited outside
          Trove. Pick how to resolve; Trove will not silently overwrite your changes.
        </p>
      </div>

      {mode === 'three-way' ? (
        <Pane
          title="Original; what Trove last wrote"
          payload={conflict.originalRegionPayload ?? ''}
          mutedTone
          testId="conflict-pane-original"
        />
      ) : (
        <p
          className="rounded-card border border-hairline bg-canvas px-3 py-2 text-[12px] text-fg-secondary dark:border-hairline-dark dark:bg-canvas-dark dark:text-fg-secondary-dark"
          data-testid="conflict-orphan-notice"
        >
          No prior baseline is on file for this harness; Trove can&apos;t show what it last wrote.
          Compare your version against Trove&apos;s intended write below.
        </p>
      )}

      <DiffPane
        title="Yours; what's in the file now"
        baseline={conflict.originalRegionPayload}
        payload={conflict.currentRegionPayload}
        testId="conflict-pane-yours"
      />

      <DiffPane
        title="Trove's; what apply would write"
        baseline={conflict.originalRegionPayload}
        payload={conflict.theirsRegionPayload}
        testId="conflict-pane-theirs"
      />

      {actionError ? (
        <div
          className="flex items-start gap-2 rounded-card border border-hairline bg-ios-red/[0.08] px-3 py-2 text-[13px] text-fg-primary dark:border-hairline-dark dark:text-fg-primary-dark"
          data-testid="conflict-action-error"
        >
          <StatusDot status="red" size="md" pulse={false} className="mt-1" />
          <div>
            <p className="font-medium">Resolution failed</p>
            <p className="mt-1 text-[12px] text-fg-secondary dark:text-fg-secondary-dark">
              {actionError.cause.kind}
            </p>
          </div>
        </div>
      ) : null}

      <div className="flex flex-wrap items-center justify-end gap-2 border-t border-hairline pt-3 dark:border-hairline-dark">
        <Button variant="secondary" size="md" testid="conflict-cancel" onClick={onCancel}>
          Cancel
        </Button>
        <Button
          variant="secondary"
          size="md"
          testid="conflict-merge-manually"
          disabled={pending !== null}
          onClick={() => void dispatch({ kind: 'merge-manually', options })}
        >
          {pending === 'merge-manually' ? 'Writing siblings…' : 'Merge manually'}
        </Button>
        <Button
          variant="secondary"
          size="md"
          testid="conflict-keep-mine"
          disabled={pending !== null}
          onClick={() => void dispatch({ kind: 'keep-mine' })}
        >
          {pending === 'keep-mine' ? 'Saving baseline…' : 'Keep mine'}
        </Button>
        <Button
          variant="primary"
          size="md"
          testid="conflict-take-theirs"
          disabled={pending !== null}
          onClick={() => void dispatch({ kind: 'take-theirs', options })}
        >
          {pending === 'take-theirs' ? 'Overwriting…' : "Take Trove's"}
        </Button>
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

function Pane({ title, payload, mutedTone, testId }: PaneProps): JSX.Element {
  return (
    <section data-testid={testId}>
      <h3 className="mb-1 text-caption uppercase text-fg-tertiary dark:text-fg-tertiary-dark">
        {title}
      </h3>
      <pre
        className={
          mutedTone
            ? 'max-h-[20vh] overflow-auto rounded-card border border-hairline bg-canvas p-3 text-[12px] text-fg-tertiary dark:border-hairline-dark dark:bg-canvas-dark dark:text-fg-tertiary-dark'
            : 'max-h-[20vh] overflow-auto rounded-card border border-hairline bg-canvas p-3 text-[12px] leading-snug dark:border-hairline-dark dark:bg-canvas-dark'
        }
      >
        {payload || (
          <span className="italic text-fg-tertiary dark:text-fg-tertiary-dark">(empty)</span>
        )}
      </pre>
    </section>
  );
}

interface DiffPaneProps {
  title: string;
  baseline: string | null;
  payload: string;
  testId: string;
}

function DiffPane({ title, baseline, payload, testId }: DiffPaneProps): JSX.Element {
  if (baseline === null) {
    return <Pane title={title} payload={payload} testId={testId} />;
  }
  const parts = diffLines(baseline, payload);
  return (
    <section data-testid={testId}>
      <h3 className="mb-1 text-caption uppercase text-fg-tertiary dark:text-fg-tertiary-dark">
        {title}
      </h3>
      <pre className="max-h-[24vh] overflow-auto rounded-card border border-hairline bg-canvas p-3 text-[12px] leading-snug dark:border-hairline-dark dark:bg-canvas-dark">
        {parts.map((part, idx) => (
          <span
            key={idx}
            className={
              part.added
                ? 'block bg-ios-green/[0.18] text-ios-green'
                : part.removed
                  ? 'block bg-ios-red/[0.18] text-ios-red'
                  : 'block text-fg-secondary dark:text-fg-secondary-dark'
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

function SiblingPathsBanner({
  siblingPaths,
  onCancel,
  'data-testid': testId,
}: SiblingPathsBannerProps): JSX.Element {
  return (
    <div className="space-y-3 px-5 py-4" data-testid={testId} data-state="merge-deferred">
      <div className="flex items-start gap-2 rounded-card border border-hairline bg-ios-green/[0.08] px-3 py-2 text-[13px] text-fg-primary dark:border-hairline-dark dark:text-fg-primary-dark">
        <StatusDot status="green" size="md" pulse={false} className="mt-1" />
        <p>
          Sibling files written next to your config. Open the host file in your editor and merge by
          hand. Re-apply once you&apos;re done; Trove will hash the result and either accept it (no
          conflict) or surface this resolver again.
        </p>
      </div>
      <dl className="space-y-1 font-mono text-[12px]">
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
        <Button
          variant="secondary"
          size="md"
          testid="conflict-merge-deferred-close"
          onClick={onCancel}
        >
          Close
        </Button>
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
      <dt className="text-[10px] uppercase tracking-wide text-fg-tertiary dark:text-fg-tertiary-dark">
        {label}
      </dt>
      <dd className="break-all text-fg-primary dark:text-fg-primary-dark" data-testid={testId}>
        {value}
      </dd>
    </div>
  );
}

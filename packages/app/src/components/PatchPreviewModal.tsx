import { diffLines } from 'diff';
import { useEffect, useState } from 'react';

import type { ApplyOptions, ConflictPayload, HarnessId, PatchPreview } from '@trove/shared';

import { TroveIpcError, applyPatch, previewPatch } from '../lib/ipc.js';
import { ConflictResolver } from './ConflictResolver.js';

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

export interface PatchPreviewModalProps {
  harnessId: HarnessId;
  /** Closes the modal without applying. Cancel + clean-up. */
  onClose: () => void;
  /** Called after a successful apply so the parent can refresh state. */
  onApplied: () => void;
}

/** Modal that previews the patch Trove will write to a harness's
 *  config and lets the user apply it. The diff is rendered client-side
 *  via the `diff` package; format-specific knowledge stays in Rust. */
export function PatchPreviewModal({
  harnessId,
  onClose,
  onApplied,
}: PatchPreviewModalProps): JSX.Element {
  const [preview, setPreview] = useState<PatchPreview | null>(null);
  const [loadError, setLoadError] = useState<TroveIpcError | null>(null);
  const [applyError, setApplyError] = useState<TroveIpcError | null>(null);
  const [applying, setApplying] = useState(false);
  // Sprint 8 — when apply_patch returns RegionConflictDetected, stash the
  // payload here so the modal swaps from the unified diff to the
  // resolver UI without leaving the modal lifecycle.
  const [conflict, setConflict] = useState<ConflictPayload | null>(null);

  // Sprint 3 wires both knobs to their defaults — the wizard for
  // tuning custom_attributes / log_user_prompts arrives in Sprint 5.
  const options: ApplyOptions = { logUserPrompts: false, customAttributes: {} };

  useEffect(() => {
    let cancelled = false;
    setLoadError(null);
    setApplyError(null);
    setPreview(null);
    void (async () => {
      try {
        const result = await previewPatch(harnessId, options);
        if (!cancelled) setPreview(result);
      } catch (err) {
        if (!cancelled) setLoadError(err instanceof TroveIpcError ? err : null);
      }
    })();
    return () => {
      cancelled = true;
    };
    // We deliberately depend on harnessId only — `options` is a fresh
    // object every render but its contents are constant in Sprint 3.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [harnessId]);

  const handleApply = async (): Promise<void> => {
    setApplying(true);
    setApplyError(null);
    setConflict(null);
    try {
      await applyPatch(harnessId, options);
      onApplied();
    } catch (err) {
      if (err instanceof TroveIpcError && err.cause.kind === 'region-conflict-detected') {
        setConflict(err.cause.conflict);
      } else {
        setApplyError(err instanceof TroveIpcError ? err : null);
      }
    } finally {
      setApplying(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/60 px-4"
      data-testid="patch-preview-modal"
      role="dialog"
      aria-modal="true"
    >
      <div className="w-full max-w-3xl overflow-hidden rounded-xl border border-slate-200 bg-white shadow-xl dark:border-slate-700 dark:bg-slate-900">
        <header className="flex items-start justify-between gap-4 border-b border-slate-200 px-5 py-4 dark:border-slate-800">
          <div>
            <h2 className="text-lg font-semibold text-slate-900 dark:text-slate-100">
              Apply Trove patch — {HARNESS_LABELS[harnessId]}
            </h2>
            {preview ? (
              <p className="mt-1 text-xs text-slate-500 dark:text-slate-400">
                {preview.configPath}
              </p>
            ) : null}
          </div>
          <button
            type="button"
            onClick={onClose}
            className="text-sm text-slate-500 hover:text-slate-900 dark:hover:text-slate-100"
            aria-label="close-modal"
          >
            ✕
          </button>
        </header>

        {conflict ? (
          <div className="max-h-[70vh] overflow-auto">
            <ConflictResolver
              harnessId={harnessId}
              conflict={conflict}
              options={options}
              onResolved={(outcome) => {
                if (outcome.status !== 'merge-deferred') {
                  onApplied();
                }
              }}
              onCancel={onClose}
            />
          </div>
        ) : (
          <>
            <div className="max-h-[60vh] overflow-auto px-5 py-4">
              {loadError ? (
                <ErrorBanner err={loadError} title="Could not preview the patch" />
              ) : preview === null ? (
                <p
                  className="text-sm text-slate-500 dark:text-slate-400"
                  data-testid="patch-preview-loading"
                >
                  Computing diff…
                </p>
              ) : (
                <DiffView preview={preview} />
              )}
            </div>

            {applyError ? (
              <div className="px-5 pb-3">
                <ErrorBanner err={applyError} title="Apply failed" />
              </div>
            ) : null}

            <footer className="flex items-center justify-end gap-3 border-t border-slate-200 px-5 py-3 dark:border-slate-800">
              <button
                type="button"
                onClick={onClose}
                className="rounded-md border border-slate-300 bg-white px-3 py-1.5 text-sm text-slate-900 transition hover:bg-slate-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-100 dark:hover:bg-slate-800"
              >
                Cancel
              </button>
              <button
                type="button"
                onClick={() => void handleApply()}
                disabled={applying || preview === null || loadError !== null}
                className="rounded-md bg-slate-900 px-3 py-1.5 text-sm font-medium text-white transition hover:bg-slate-700 disabled:cursor-not-allowed disabled:opacity-50 dark:bg-slate-100 dark:text-slate-900 dark:hover:bg-slate-200"
                data-testid="patch-preview-apply"
              >
                {applying ? 'Applying…' : preview?.status === 'idempotent' ? 'Re-apply' : 'Apply'}
              </button>
            </footer>
          </>
        )}
      </div>
    </div>
  );
}

interface DiffViewProps {
  preview: PatchPreview;
}

function DiffView({ preview }: DiffViewProps): JSX.Element {
  if (preview.status === 'conflict') {
    return (
      <div data-testid="patch-preview-conflict" className="space-y-3">
        <p className="rounded-md border border-amber-300 bg-amber-50 px-3 py-2 text-sm text-amber-900 dark:border-amber-700 dark:bg-amber-950 dark:text-amber-100">
          A managed Trove block already exists in this file but doesn&apos;t match the patch we
          would write — it looks like the file was edited outside Trove. Click Apply to open the
          3-way merge resolver and decide how to reconcile the changes.
        </p>
        <DiffPre before={preview.before} after={preview.after} />
      </div>
    );
  }
  if (preview.status === 'idempotent') {
    return (
      <div data-testid="patch-preview-idempotent" className="space-y-3">
        <p className="rounded-md border border-emerald-300 bg-emerald-50 px-3 py-2 text-sm text-emerald-900 dark:border-emerald-700 dark:bg-emerald-950 dark:text-emerald-100">
          The current file already matches what Trove would write. Re-applying is a no-op.
        </p>
        <DiffPre before={preview.before} after={preview.after} />
      </div>
    );
  }
  return (
    <div data-testid="patch-preview-fresh">
      <DiffPre before={preview.before} after={preview.after} />
    </div>
  );
}

interface DiffPreProps {
  before: string;
  after: string;
}

function DiffPre({ before, after }: DiffPreProps): JSX.Element {
  const parts = diffLines(before, after);
  return (
    <pre className="max-h-[40vh] overflow-auto rounded-md border border-slate-200 bg-slate-50 p-3 text-xs leading-snug dark:border-slate-800 dark:bg-slate-950">
      {parts.map((part, idx) => (
        <span
          // diffLines returns stable parts; the index is a fine key
          // for a non-reordered list.
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
  );
}

interface ErrorBannerProps {
  err: TroveIpcError | null;
  title: string;
}

function ErrorBanner({ err, title }: ErrorBannerProps): JSX.Element {
  return (
    <div
      className="rounded-md border border-red-300 bg-red-50 px-3 py-2 text-sm text-red-900 dark:border-red-700 dark:bg-red-950 dark:text-red-200"
      data-testid="patch-preview-error"
    >
      <p className="font-medium">{title}</p>
      <p className="mt-1 text-xs">{err ? describeIpcError(err) : 'unknown error'}</p>
    </div>
  );
}

function describeIpcError(err: TroveIpcError): string {
  const cause = err.cause;
  switch (cause.kind) {
    case 'config-unparseable':
      return `${cause.path}: ${cause.reason}`;
    case 'region-conflict':
      return `${cause.path} contains a Trove region that doesn't match this patch.`;
    case 'region-conflict-detected':
      // Sprint 8 — the resolver UI consumes the structured payload
      // directly, so this string is only a fallback for surfaces that
      // don't render the resolver yet.
      return `${cause.conflict.configPath}: 3-way conflict — choose Keep mine / Take Trove's / Merge manually.`;
    case 'harness-not-detected':
      return `${cause.id} is not detected on this machine.`;
    case 'harness-not-implemented':
      return `${cause.id} adapter not yet implemented.`;
    case 'io':
      return `${cause.path}: ${cause.reason}`;
    case 'internal':
      return cause.reason;
  }
}

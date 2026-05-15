import { diffLines } from 'diff';
import { useEffect, useState } from 'react';

import type { ApplyOptions, ConflictPayload, HarnessId, PatchPreview } from '@trove/shared';

import { TroveIpcError, applyPatch, previewPatch } from '../lib/ipc.js';
import { ConflictResolver } from './ConflictResolver.js';
import { Button, Sheet, StatusDot } from './ui/index.js';

const HARNESS_LABELS: Record<HarnessId, string> = {
  'claude-code': 'Claude Code',
  'claude-desktop': 'Claude Desktop',
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
  onClose: () => void;
  onApplied: () => void;
}

export function PatchPreviewModal({
  harnessId,
  onClose,
  onApplied,
}: PatchPreviewModalProps): JSX.Element {
  const [preview, setPreview] = useState<PatchPreview | null>(null);
  const [loadError, setLoadError] = useState<TroveIpcError | null>(null);
  const [applyError, setApplyError] = useState<TroveIpcError | null>(null);
  const [applying, setApplying] = useState(false);
  const [conflict, setConflict] = useState<ConflictPayload | null>(null);

  const options: ApplyOptions = { customAttributes: {} };

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

  const title = `Apply Trove patch — ${HARNESS_LABELS[harnessId]}`;
  const subtitle = preview ? preview.configPath : undefined;

  return (
    <Sheet
      open
      onClose={onClose}
      title={title}
      subtitle={subtitle}
      size="lg"
      testid="patch-preview-modal"
      footer={
        conflict ? undefined : (
          <>
            <Button variant="secondary" size="md" onClick={onClose}>
              Cancel
            </Button>
            <Button
              variant="primary"
              size="md"
              testid="patch-preview-apply"
              loading={applying}
              disabled={applying || preview === null || loadError !== null}
              onClick={() => void handleApply()}
            >
              {applying ? 'Applying…' : preview?.status === 'idempotent' ? 'Re-apply' : 'Apply'}
            </Button>
          </>
        )
      }
    >
      {conflict ? (
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
      ) : (
        <>
          <div className="px-5 py-4">
            {loadError ? (
              <ErrorBanner err={loadError} title="Could not preview the patch" />
            ) : preview === null ? (
              <p
                className="text-[13px] text-fg-secondary dark:text-fg-secondary-dark"
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
        </>
      )}
    </Sheet>
  );
}

interface DiffViewProps {
  preview: PatchPreview;
}

function DiffView({ preview }: DiffViewProps): JSX.Element {
  if (preview.status === 'conflict') {
    return (
      <div data-testid="patch-preview-conflict" className="space-y-3">
        <div className="flex items-start gap-2 rounded-card border border-hairline bg-ios-orange/[0.08] px-3 py-2 text-[13px] text-fg-primary dark:border-hairline-dark dark:text-fg-primary-dark">
          <StatusDot status="amber" size="md" pulse={false} className="mt-1" />
          <p>
            A managed Trove block already exists in this file but doesn&apos;t match the patch we
            would write — it looks like the file was edited outside Trove. Click Apply to open the
            3-way merge resolver and decide how to reconcile the changes.
          </p>
        </div>
        <DiffPre before={preview.before} after={preview.after} />
      </div>
    );
  }
  if (preview.status === 'idempotent') {
    return (
      <div data-testid="patch-preview-idempotent" className="space-y-3">
        <div className="flex items-start gap-2 rounded-card border border-hairline bg-ios-green/[0.08] px-3 py-2 text-[13px] text-fg-primary dark:border-hairline-dark dark:text-fg-primary-dark">
          <StatusDot status="green" size="md" pulse={false} className="mt-1" />
          <p>The current file already matches what Trove would write. Re-applying is a no-op.</p>
        </div>
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
    <pre className="max-h-[40vh] overflow-auto rounded-card border border-hairline bg-canvas p-3 text-[12px] leading-snug dark:border-hairline-dark dark:bg-canvas-dark">
      {/* inline-block wrapper expands to the widest line so block-level
          line highlights paint across the full scroll width, not just
          the visible viewport. */}
      <div className="inline-block min-w-full">
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
      </div>
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
      className="flex items-start gap-2 rounded-card border border-hairline bg-ios-red/[0.08] px-3 py-2 text-[13px] text-fg-primary dark:border-hairline-dark dark:text-fg-primary-dark"
      data-testid="patch-preview-error"
    >
      <StatusDot status="red" size="md" pulse={false} className="mt-1" />
      <div>
        <p className="font-medium">{title}</p>
        <p className="mt-1 text-[12px] text-fg-secondary dark:text-fg-secondary-dark">
          {err ? describeIpcError(err) : 'unknown error'}
        </p>
      </div>
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
      return `${cause.conflict.configPath}: 3-way conflict — choose Keep mine / Take Trove's / Merge manually.`;
    case 'harness-not-detected':
      return `${cause.id} is not detected on this machine.`;
    case 'harness-not-implemented':
      return `${cause.id} adapter not yet implemented.`;
    case 'io':
      return `${cause.path}: ${cause.reason}`;
    case 'updater-check-failed':
      return `update check failed: ${cause.reason}`;
    case 'internal':
      return cause.reason;
  }
}

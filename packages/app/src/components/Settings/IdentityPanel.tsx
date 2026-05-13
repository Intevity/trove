import { useCallback, useEffect, useState } from 'react';

import type { ResolvedIdentity, ResolvedIdentitySource } from '@trove/shared';

import {
  TroveIpcError,
  resolveIdentityPreview,
  setIdentityAuto,
  setIdentityEnabled,
  setIdentityManual,
} from '../../lib/ipc.js';
import { Button, Card, CardHeader, CardTitle } from '../ui/index.js';

interface Props {
  enabled: boolean;
  manualName: string;
  manualEmail: string;
  onChanged: () => void | Promise<void>;
}

export function IdentityPanel({ enabled, manualName, manualEmail, onChanged }: Props): JSX.Element {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<TroveIpcError | null>(null);
  const [preview, setPreview] = useState<ResolvedIdentity | null>(null);
  const [expanded, setExpanded] = useState(false);
  const [draftName, setDraftName] = useState(manualName);
  const [draftEmail, setDraftEmail] = useState(manualEmail);

  const loadPreview = useCallback(async () => {
    try {
      const r = await resolveIdentityPreview();
      setPreview(r);
    } catch (err) {
      if (err instanceof TroveIpcError) setError(err);
    }
  }, []);

  useEffect(() => {
    void loadPreview();
  }, [loadPreview]);

  useEffect(() => {
    setDraftName(manualName);
    setDraftEmail(manualEmail);
  }, [manualName, manualEmail]);

  const handleToggle = async (e: React.ChangeEvent<HTMLInputElement>): Promise<void> => {
    setBusy(true);
    setError(null);
    try {
      await setIdentityEnabled(e.target.checked);
      await onChanged();
      await loadPreview();
    } catch (err) {
      if (err instanceof TroveIpcError) setError(err);
    } finally {
      setBusy(false);
    }
  };

  const handleSaveManual = async (): Promise<void> => {
    setBusy(true);
    setError(null);
    try {
      await setIdentityManual(draftName.trim(), draftEmail.trim());
      await onChanged();
      await loadPreview();
    } catch (err) {
      if (err instanceof TroveIpcError) setError(err);
    } finally {
      setBusy(false);
    }
  };

  const handleUseAuto = async (): Promise<void> => {
    setBusy(true);
    setError(null);
    try {
      await setIdentityAuto();
      await onChanged();
      await loadPreview();
    } catch (err) {
      if (err instanceof TroveIpcError) setError(err);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card testid="identity-panel">
      <CardHeader>
        <CardTitle>Identity tagging</CardTitle>
      </CardHeader>

      <label className="flex items-start gap-3 text-[13px] text-fg-secondary dark:text-fg-secondary-dark">
        <input
          type="checkbox"
          data-testid="identity-toggle"
          checked={enabled}
          disabled={busy}
          onChange={(e) => void handleToggle(e)}
          className="mt-0.5 h-4 w-4 rounded border-fg-tertiary focus:ring-ios-blue dark:border-fg-tertiary-dark"
        />
        <span className="flex flex-col">
          <span className="text-fg-primary dark:text-fg-primary-dark">
            Tag outgoing telemetry with my name and email
          </span>
          <span className="text-[12px] text-fg-tertiary dark:text-fg-tertiary-dark">
            Off by default. When on, every signal carries{' '}
            <code className="font-mono text-[11px]">user.name</code> and{' '}
            <code className="font-mono text-[11px]">user.email</code> resource attributes and lands
            on your observability platform.
          </span>
        </span>
      </label>

      {preview ? (
        <div
          data-testid="identity-preview"
          className="mt-3 rounded-card border border-hairline bg-canvas px-3 py-2 text-[12px] text-fg-secondary dark:border-hairline-dark dark:bg-canvas-dark dark:text-fg-secondary-dark"
        >
          <p>
            <span className="font-medium text-fg-primary dark:text-fg-primary-dark">Resolved:</span>{' '}
            {describeResolved(preview)}
          </p>
          <p className="mt-0.5">
            <span className="font-medium text-fg-primary dark:text-fg-primary-dark">Source:</span>{' '}
            {describeSource(preview.source)}
          </p>
        </div>
      ) : null}

      <div className="mt-3 flex items-center gap-3">
        <Button
          variant="ghost"
          size="sm"
          testid="identity-expand-override"
          onClick={() => setExpanded((s) => !s)}
        >
          {expanded ? 'Hide override' : 'Override manually…'}
        </Button>
        {preview?.source.kind === 'manual' ? (
          <Button
            variant="ghost"
            size="sm"
            testid="identity-use-auto"
            disabled={busy}
            onClick={() => void handleUseAuto()}
          >
            Use auto-detected values
          </Button>
        ) : null}
      </div>

      {expanded ? (
        <div
          data-testid="identity-override-form"
          className="mt-2 grid grid-cols-[max-content_1fr] items-center gap-x-3 gap-y-2 text-[12px] text-fg-secondary dark:text-fg-secondary-dark"
        >
          <label htmlFor="identity-name" className="text-fg-primary dark:text-fg-primary-dark">
            Name
          </label>
          <input
            id="identity-name"
            data-testid="identity-name-input"
            type="text"
            value={draftName}
            onChange={(e) => setDraftName(e.target.value)}
            className="rounded-[8px] border border-hairline bg-surface-elevated px-2 py-1 text-[13px] text-fg-primary focus:border-ios-blue focus:outline-none focus:ring-1 focus:ring-ios-blue dark:border-hairline-dark dark:bg-surface-elevated-dark dark:text-fg-primary-dark"
          />
          <label htmlFor="identity-email" className="text-fg-primary dark:text-fg-primary-dark">
            Email
          </label>
          <input
            id="identity-email"
            data-testid="identity-email-input"
            type="email"
            value={draftEmail}
            onChange={(e) => setDraftEmail(e.target.value)}
            className="rounded-[8px] border border-hairline bg-surface-elevated px-2 py-1 text-[13px] text-fg-primary focus:border-ios-blue focus:outline-none focus:ring-1 focus:ring-ios-blue dark:border-hairline-dark dark:bg-surface-elevated-dark dark:text-fg-primary-dark"
          />
          <span />
          <div className="flex justify-end">
            <Button
              variant="primary"
              size="sm"
              testid="identity-save-override"
              disabled={busy}
              onClick={() => void handleSaveManual()}
            >
              Save override
            </Button>
          </div>
        </div>
      ) : null}

      {error ? (
        <p data-testid="identity-error" className="mt-2 text-[12px] text-ios-red">
          {error.cause.kind === 'internal'
            ? `Failed: ${error.cause.reason}`
            : `Failed: ${error.cause.kind}`}
        </p>
      ) : null}
    </Card>
  );
}

function describeResolved(preview: ResolvedIdentity): string {
  if (preview.source.kind === 'none') return 'nothing would be tagged';
  const parts: string[] = [];
  if (preview.name) parts.push(`user.name=${preview.name}`);
  if (preview.email) parts.push(`user.email=${preview.email}`);
  return parts.length > 0 ? parts.join(', ') : 'nothing would be tagged';
}

function describeSource(source: ResolvedIdentitySource): string {
  switch (source.kind) {
    case 'manual':
      return 'manual entry';
    case 'harness':
      return `detected from ${source.id} config`;
    case 'git-config':
      return 'git config (--global)';
    case 'none':
      return 'no probe returned values; tagging is a no-op';
  }
}

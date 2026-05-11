import { useCallback, useEffect, useState } from 'react';

import type { ResolvedIdentity, ResolvedIdentitySource } from '@trove/shared';

import {
  TroveIpcError,
  resolveIdentityPreview,
  setIdentityAuto,
  setIdentityEnabled,
  setIdentityManual,
} from '../../lib/ipc.js';

interface Props {
  /** Whether the persisted `identity.enabled` flag is on. The parent
   *  passes `appState.identity.enabled`. */
  enabled: boolean;
  /** Persisted manual values; pre-fills the override inputs. */
  manualName: string;
  manualEmail: string;
  /** Called after a mutation persists so the parent re-fetches
   *  appState. */
  onChanged: () => void | Promise<void>;
}

/** Sprint 12 — opt-in identity tagging section in the Dashboard.
 *
 *  Surfaces a single toggle and a preview of the values that would
 *  be attached to outgoing telemetry. Off by default. When enabled,
 *  the collector tags every signal with `user.name` and `user.email`
 *  resource attributes resolved via the probe ladder in
 *  `crate::identity::resolve` (harness → git config → manual
 *  override). An expanded form lets the user override the auto-
 *  resolved values verbatim. */
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
    <section
      data-testid="identity-panel"
      className="rounded-md border border-slate-200 bg-white px-4 py-3 dark:border-slate-800 dark:bg-slate-900"
    >
      <header className="mb-2">
        <h2 className="text-sm font-semibold text-slate-900 dark:text-slate-100">
          Identity tagging
        </h2>
      </header>

      <label className="flex items-start gap-3 text-sm text-slate-700 dark:text-slate-300">
        <input
          type="checkbox"
          data-testid="identity-toggle"
          checked={enabled}
          disabled={busy}
          onChange={(e) => void handleToggle(e)}
          className="mt-0.5 h-4 w-4 rounded border-slate-400 text-slate-900 focus:ring-slate-500 dark:border-slate-600"
        />
        <span className="flex flex-col">
          <span>Tag outgoing telemetry with my name and email</span>
          <span className="text-xs text-slate-500 dark:text-slate-400">
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
          className="mt-3 rounded-md border border-slate-200 bg-slate-50 px-3 py-2 text-xs text-slate-700 dark:border-slate-700 dark:bg-slate-950 dark:text-slate-300"
        >
          <p>
            <span className="font-medium">Resolved:</span> {describeResolved(preview)}
          </p>
          <p className="mt-0.5">
            <span className="font-medium">Source:</span> {describeSource(preview.source)}
          </p>
        </div>
      ) : null}

      <div className="mt-3 flex items-center gap-3">
        <button
          type="button"
          data-testid="identity-expand-override"
          onClick={() => setExpanded((s) => !s)}
          className="text-xs text-blue-700 hover:underline dark:text-blue-300"
        >
          {expanded ? 'Hide override' : 'Override manually…'}
        </button>
        {preview?.source.kind === 'manual' ? (
          <button
            type="button"
            data-testid="identity-use-auto"
            disabled={busy}
            onClick={() => void handleUseAuto()}
            className="text-xs text-slate-700 hover:underline disabled:opacity-50 dark:text-slate-300"
          >
            Use auto-detected values
          </button>
        ) : null}
      </div>

      {expanded ? (
        <div
          data-testid="identity-override-form"
          className="mt-2 grid grid-cols-[max-content_1fr] items-center gap-x-3 gap-y-2 text-xs text-slate-700 dark:text-slate-300"
        >
          <label htmlFor="identity-name">Name</label>
          <input
            id="identity-name"
            data-testid="identity-name-input"
            type="text"
            value={draftName}
            onChange={(e) => setDraftName(e.target.value)}
            className="rounded border border-slate-300 bg-white px-2 py-1 text-sm text-slate-900 dark:border-slate-700 dark:bg-slate-950 dark:text-slate-100"
          />
          <label htmlFor="identity-email">Email</label>
          <input
            id="identity-email"
            data-testid="identity-email-input"
            type="email"
            value={draftEmail}
            onChange={(e) => setDraftEmail(e.target.value)}
            className="rounded border border-slate-300 bg-white px-2 py-1 text-sm text-slate-900 dark:border-slate-700 dark:bg-slate-950 dark:text-slate-100"
          />
          <span />
          <div className="flex justify-end">
            <button
              type="button"
              data-testid="identity-save-override"
              disabled={busy}
              onClick={() => void handleSaveManual()}
              className="rounded-md bg-emerald-600 px-3 py-1 text-xs font-medium text-white hover:bg-emerald-700 disabled:cursor-not-allowed disabled:opacity-50"
            >
              Save override
            </button>
          </div>
        </div>
      ) : null}

      {error ? (
        <p data-testid="identity-error" className="mt-2 text-xs text-red-700 dark:text-red-300">
          {error.cause.kind === 'internal'
            ? `Failed: ${error.cause.reason}`
            : `Failed: ${error.cause.kind}`}
        </p>
      ) : null}
    </section>
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

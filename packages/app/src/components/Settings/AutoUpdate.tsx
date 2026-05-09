import { useState } from 'react';

import type { UpdateMetadata } from '@trove/shared';

import { TroveIpcError, checkForUpdates } from '../../lib/ipc.js';

interface Props {
  /** Whether the persisted `autoUpdateEnabled` flag is on. The parent
   *  passes the value from `appState.autoUpdateEnabled`. */
  enabled: boolean;
  /** Called when the user toggles the checkbox. The parent persists
   *  via `setAutoUpdateEnabled` and refreshes app state. */
  onToggle: (next: boolean) => void | Promise<void>;
}

/** Sprint 10 — opt-in auto-updater section in the Dashboard.
 *
 *  Two affordances:
 *  - A checkbox that toggles `autoUpdateEnabled` (gates the
 *    background-on-launch probe; default off).
 *  - A "Check for updates now…" button that always runs (an explicit
 *    user action is not "surprise network activity").
 *
 *  Trove never contacts GitHub Releases without one of these — the
 *  flag set, or the button clicked. */
export function AutoUpdate({ enabled, onToggle }: Props): JSX.Element {
  const [checking, setChecking] = useState(false);
  const [result, setResult] = useState<UpdateMetadata | null>(null);
  const [error, setError] = useState<TroveIpcError | null>(null);

  async function handleCheck(): Promise<void> {
    setChecking(true);
    setError(null);
    setResult(null);
    try {
      const r = await checkForUpdates();
      setResult(r);
    } catch (err) {
      if (err instanceof TroveIpcError) setError(err);
    } finally {
      setChecking(false);
    }
  }

  function handleToggle(e: React.ChangeEvent<HTMLInputElement>): void {
    void onToggle(e.target.checked);
  }

  return (
    <section
      data-testid="auto-update-panel"
      className="rounded-md border border-slate-200 bg-white px-4 py-3 dark:border-slate-800 dark:bg-slate-900"
    >
      <header className="mb-2">
        <h2 className="text-sm font-semibold text-slate-900 dark:text-slate-100">Updates</h2>
      </header>

      <label className="flex items-start gap-3 text-sm text-slate-700 dark:text-slate-300">
        <input
          type="checkbox"
          data-testid="auto-update-toggle"
          checked={enabled}
          onChange={handleToggle}
          className="mt-0.5 h-4 w-4 rounded border-slate-400 text-slate-900 focus:ring-slate-500 dark:border-slate-600"
        />
        <span className="flex flex-col">
          <span>Automatically check for updates</span>
          <span className="text-xs text-slate-500 dark:text-slate-400">
            Off by default. Trove only contacts GitHub Releases when this is on, or when you click
            the button below.
          </span>
        </span>
      </label>

      <div className="mt-3 flex items-center gap-3">
        <button
          type="button"
          data-testid="auto-update-check-now"
          onClick={() => void handleCheck()}
          disabled={checking}
          className="rounded-md border border-slate-300 bg-white px-3 py-1.5 text-xs font-medium text-slate-900 transition hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-100 dark:hover:bg-slate-800"
        >
          {checking ? 'Checking…' : 'Check for updates now…'}
        </button>
        <span
          data-testid="auto-update-status"
          className="text-xs text-slate-600 dark:text-slate-400"
        >
          {renderStatus(result, error)}
        </span>
      </div>
    </section>
  );
}

function renderStatus(result: UpdateMetadata | null, error: TroveIpcError | null): string {
  if (error) {
    if (error.cause.kind === 'updater-check-failed') {
      return `Check failed: ${error.cause.reason}`;
    }
    return `Check failed: ${error.cause.kind}`;
  }
  if (!result) return '';
  if (result.available && result.version) {
    return `Update available: v${result.version} (running v${result.current}).`;
  }
  return `You're on v${result.current} — up to date.`;
}

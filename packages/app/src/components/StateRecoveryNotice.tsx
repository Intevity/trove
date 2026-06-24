import { AlertTriangle, Loader2 } from 'lucide-react';
import { useCallback, useState } from 'react';

import type { TroveIpcError } from '../lib/ipc.js';
import { checkForUpdates } from '../lib/ipc.js';
import { Button } from './ui/index.js';

interface Props {
  /** The error that caused `getAppState` to fail. */
  error: TroveIpcError;
  /** Re-attempt the state load (e.g. after the user updates Trove). */
  onRetry: () => void;
}

/** Shown when `state.json` exists but this build can't load it — most
 *  importantly when the file was written by a NEWER Trove build (the
 *  downgrade case). This is deliberately NOT the first-run wizard: the
 *  user's data is intact on disk, and presenting the wizard would imply
 *  it was wiped. We explain what happened, point at the on-disk file, and
 *  offer a path forward (update + retry) without ever touching the data. */
export function StateRecoveryNotice({ error, onRetry }: Props): JSX.Element {
  const [checking, setChecking] = useState(false);
  const [checkResult, setCheckResult] = useState<string | null>(null);

  const onCheckUpdates = useCallback(async () => {
    setChecking(true);
    setCheckResult(null);
    try {
      const meta = await checkForUpdates();
      setCheckResult(
        meta.available && meta.version
          ? `Trove v${meta.version} is available — install it, then retry.`
          : "No newer release is published yet. If you're on a local dev build, rebuild from source.",
      );
    } catch {
      setCheckResult('Update check failed. Check your connection and try again.');
    } finally {
      setChecking(false);
    }
  }, []);

  const cause = error.cause;
  const isNewer = cause.kind === 'state-from-newer-version';

  return (
    <main
      data-testid="state-recovery-notice"
      className="h-full overflow-y-auto text-slate-900 antialiased dark:text-slate-100"
    >
      <div className="mx-auto flex max-w-3xl flex-col items-stretch gap-6 px-8 py-12">
        <header className="flex items-start gap-3">
          <AlertTriangle className="mt-0.5 h-6 w-6 shrink-0 text-ios-orange" aria-hidden />
          <div>
            <h1 className="text-3xl font-semibold tracking-tight">Your data is safe</h1>
            <p className="mt-1 text-sm text-fg-secondary dark:text-fg-secondary-dark">
              {isNewer
                ? 'Trove found a configuration created by a newer version than the one running now, so it left the file untouched rather than risk overwriting it.'
                : "Trove couldn't read your saved configuration. It left the file untouched on disk."}
            </p>
          </div>
        </header>

        <section className="rounded-tile border border-hairline bg-surface p-4 text-[13px] text-fg-primary dark:border-hairline-dark dark:bg-surface-dark dark:text-fg-primary-dark">
          {isNewer ? (
            <dl className="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-1.5">
              <dt className="text-fg-secondary dark:text-fg-secondary-dark">Created by</dt>
              <dd>Trove schema v{cause.found}</dd>
              <dt className="text-fg-secondary dark:text-fg-secondary-dark">This build reads</dt>
              <dd>up to schema v{cause.expected}</dd>
              <dt className="text-fg-secondary dark:text-fg-secondary-dark">Data file</dt>
              <dd className="break-all font-mono text-[12px]">{cause.path}</dd>
            </dl>
          ) : (
            <p className="font-mono text-[12px] break-all">{error.message}</p>
          )}
          <p className="mt-3 text-fg-secondary dark:text-fg-secondary-dark">
            {isNewer
              ? 'Update Trove to this build or newer to open your existing setup. Nothing has been deleted.'
              : 'Update or reinstall Trove, then retry. Nothing has been deleted.'}
          </p>
        </section>

        <div className="flex items-center gap-2">
          <Button
            variant="primary"
            size="md"
            onClick={() => void onCheckUpdates()}
            loading={checking}
          >
            {checking ? (
              <>
                <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden />
                Checking…
              </>
            ) : (
              'Check for updates'
            )}
          </Button>
          <Button variant="secondary" size="md" onClick={onRetry}>
            Retry
          </Button>
        </div>

        {checkResult !== null && (
          <p className="text-[13px] text-fg-secondary dark:text-fg-secondary-dark">{checkResult}</p>
        )}
      </div>
    </main>
  );
}

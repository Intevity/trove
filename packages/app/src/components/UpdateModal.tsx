import { invoke } from '@tauri-apps/api/core';
import { ArrowDownToLine, Loader2, X } from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';

import { Button } from './ui/index.js';

export interface UpdateModalProps {
  /** Version announced by the update endpoint (no leading "v"). */
  version: string;
  /** Version currently running (no leading "v"). */
  currentVersion: string;
  onClose: () => void;
}

/** "Update available" dialog raised by the Rust updater's
 *  `update_available` event (tray "Check for updates…" item, the
 *  Settings "Check for updates now…" button, or the 4-hourly background
 *  check). Nothing installs until the user clicks Install: the Rust side
 *  stashes the found update and the `install_update` command consumes
 *  it, downloads, installs, and restarts the app. Dismissing is cheap;
 *  the next check raises the dialog again.
 *
 *  `install_update` is invoked raw (not via the `lib/ipc.js` wrapper):
 *  on success the app restarts and the promise never settles, so there
 *  is no response payload to validate — only the failure string. */
export function UpdateModal({ version, currentVersion, onClose }: UpdateModalProps): JSX.Element {
  const [installing, setInstalling] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Escape closes, but not mid-install: the restart is imminent and the
  // dialog is the only progress signal.
  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === 'Escape' && !installing) onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [installing, onClose]);

  const install = useCallback(async (): Promise<void> => {
    setInstalling(true);
    setError(null);
    try {
      // On success the app restarts and this promise never settles in a
      // surviving webview; the catch below only runs on failure.
      await invoke('install_update');
    } catch (e) {
      setError(typeof e === 'string' ? e : 'Update failed. Please try again later.');
      setInstalling(false);
    }
  }, []);

  return (
    <div
      data-testid="update-modal"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4 backdrop-blur-sm"
      role="dialog"
      aria-modal="true"
      aria-label="Update available"
      onClick={() => {
        if (!installing) onClose();
      }}
    >
      <div
        className="w-[360px] max-w-[92vw] rounded-tile border border-hairline bg-surface-elevated p-5 shadow-modal dark:border-hairline-dark dark:bg-surface-elevated-dark dark:shadow-modal-dark"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-2 flex items-start justify-between gap-3">
          <h3 className="text-[14px] font-semibold text-fg-primary dark:text-fg-primary-dark">
            Update available
          </h3>
          {!installing && (
            <button
              type="button"
              aria-label="Close"
              data-testid="update-modal-close"
              onClick={onClose}
              className="shrink-0 rounded-[6px] p-0.5 text-fg-tertiary hover:bg-black/[0.04] hover:text-fg-primary dark:text-fg-tertiary-dark dark:hover:bg-white/[0.06] dark:hover:text-fg-primary-dark"
            >
              <X size={16} strokeWidth={2.4} aria-hidden="true" />
            </button>
          )}
        </div>
        <div className="space-y-2 text-[12px] leading-relaxed text-fg-secondary dark:text-fg-secondary-dark">
          <p>
            Trove{' '}
            <span className="font-semibold text-fg-primary dark:text-fg-primary-dark">
              v{version}
            </span>{' '}
            is ready to install. You are on v{currentVersion}.
          </p>
          <p>Trove restarts to finish the update; the collector is back within seconds.</p>
          {error !== null && <p className="text-ios-red">{error}</p>}
        </div>
        <div className="mt-4 flex items-center justify-end gap-2">
          <Button
            variant="secondary"
            size="md"
            testid="update-modal-later"
            disabled={installing}
            onClick={onClose}
          >
            Later
          </Button>
          <Button
            variant="primary"
            size="md"
            testid="update-modal-install"
            loading={installing}
            onClick={() => void install()}
          >
            {installing ? (
              <>
                <Loader2 size={13} className="animate-spin" aria-hidden="true" /> Installing…
              </>
            ) : (
              <>
                <ArrowDownToLine size={13} aria-hidden="true" /> Install and restart
              </>
            )}
          </Button>
        </div>
      </div>
    </div>
  );
}

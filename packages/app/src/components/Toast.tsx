import { X } from 'lucide-react';
import { useEffect } from 'react';

export interface ToastProps {
  /** The message body. Rendered as plain text; line breaks survive. */
  message: string;
  /** Auto-dismiss timer in milliseconds. Defaults to 15s so the user has
   *  enough time to read 1–2 sentences without rushing. Pass `0` to
   *  disable auto-dismiss (the user must click the X). */
  durationMs?: number;
  /** Fired when the auto-dismiss timer elapses or the user clicks the X.
   *  Parent owns the toast's visibility state. */
  onDismiss: () => void;
}

/** Fixed-position toast notification anchored bottom-right. One at a
 *  time — the parent renders 0 or 1, never a stack — which matches the
 *  single-event surfaces in Trove today (harness Enable / Disable). If
 *  we ever need a stack, lift the state into a provider and render a
 *  list of these; the per-toast shell stays the same. */
export function Toast({ message, durationMs = 15_000, onDismiss }: ToastProps): JSX.Element {
  useEffect(() => {
    if (durationMs <= 0) return undefined;
    const id = setTimeout(onDismiss, durationMs);
    return () => clearTimeout(id);
  }, [durationMs, onDismiss]);

  return (
    <div
      data-testid="toast"
      role="status"
      aria-live="polite"
      className="fixed bottom-4 right-4 z-50 flex max-w-sm items-start gap-3 rounded-lg border border-slate-200 bg-white px-4 py-3 shadow-lg dark:border-slate-700 dark:bg-slate-800"
    >
      <p className="flex-1 text-sm text-slate-800 dark:text-slate-100">{message}</p>
      <button
        type="button"
        data-testid="toast-dismiss"
        aria-label="Dismiss notification"
        onClick={onDismiss}
        className="flex-shrink-0 rounded p-0.5 text-slate-400 hover:bg-slate-100 hover:text-slate-700 dark:hover:bg-slate-700 dark:hover:text-slate-100"
      >
        <X size={14} aria-hidden="true" />
      </button>
    </div>
  );
}

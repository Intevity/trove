import { X } from 'lucide-react';
import { motion } from 'motion/react';
import { useEffect } from 'react';

export interface ToastProps {
  message: string;
  durationMs?: number;
  onDismiss: () => void;
}

/** Fixed-position toast notification anchored bottom-right with a
 *  soft slide-in. The Apple-native materialization: hairline ring +
 *  soft elevation, slides in from the lower-right by 12px on mount. */
export function Toast({ message, durationMs = 15_000, onDismiss }: ToastProps): JSX.Element {
  useEffect(() => {
    if (durationMs <= 0) return undefined;
    const id = setTimeout(onDismiss, durationMs);
    return () => clearTimeout(id);
  }, [durationMs, onDismiss]);

  return (
    <motion.div
      data-testid="toast"
      role="status"
      aria-live="polite"
      initial={{ opacity: 0, y: 12 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.18, ease: 'easeOut' }}
      className="fixed bottom-4 right-4 z-50 flex max-w-sm items-start gap-3 rounded-tile border border-hairline bg-surface-elevated px-4 py-3 shadow-modal dark:border-hairline-dark dark:bg-surface-elevated-dark dark:shadow-modal-dark"
    >
      <p className="flex-1 text-[13px] text-fg-primary dark:text-fg-primary-dark">{message}</p>
      <button
        type="button"
        data-testid="toast-dismiss"
        aria-label="Dismiss notification"
        onClick={onDismiss}
        className="flex-shrink-0 rounded-[6px] p-0.5 text-fg-tertiary hover:bg-black/[0.04] hover:text-fg-primary dark:text-fg-tertiary-dark dark:hover:bg-white/[0.06] dark:hover:text-fg-primary-dark"
      >
        <X size={14} aria-hidden="true" />
      </button>
    </motion.div>
  );
}

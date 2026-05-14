import { X } from 'lucide-react';
import { useEffect, type ReactNode } from 'react';

export type SheetSize = 'md' | 'lg';

export interface SheetProps {
  open: boolean;
  onClose: () => void;
  title: string;
  subtitle?: ReactNode;
  /** Pinned footer slot. Sits below the body, separated by a hairline. */
  footer?: ReactNode;
  size?: SheetSize;
  /** Forwarded to the dialog root as `data-testid`. */
  testid?: string;
  /** `role="dialog"`-friendly aria id; auto-generated when omitted. */
  labelledBy?: string;
  children?: ReactNode;
}

const SIZE_CLASS: Record<SheetSize, string> = {
  md: 'max-w-md',
  lg: 'max-w-3xl',
};

/** Apple modal sheet — frosted scrim, large radius, soft elevation,
 *  hairline-bordered header & footer slots. ESC or scrim-click closes;
 *  the inner body scrolls. */
export function Sheet({
  open,
  onClose,
  title,
  subtitle,
  footer,
  size = 'md',
  testid,
  labelledBy,
  children,
}: SheetProps): JSX.Element | null {
  useEffect(() => {
    if (!open) return undefined;
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [open, onClose]);

  if (!open) return null;

  const titleId = labelledBy ?? `sheet-title-${title.replace(/\s+/g, '-')}`;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 px-4 py-4 backdrop-blur-[6px]"
      role="dialog"
      aria-modal="true"
      aria-labelledby={titleId}
      data-testid={testid}
      onClick={onClose}
    >
      <div
        className={`flex max-h-full w-full flex-col overflow-hidden rounded-modal bg-surface-elevated shadow-modal dark:bg-surface-elevated-dark dark:shadow-modal-dark ${SIZE_CLASS[size]}`}
        onClick={(e) => e.stopPropagation()}
      >
        <header className="flex shrink-0 items-start justify-between gap-4 border-b border-hairline px-5 py-4 dark:border-hairline-dark">
          <div className="min-w-0">
            <h2
              id={titleId}
              className="text-[15px] font-semibold tracking-tight text-fg-primary dark:text-fg-primary-dark"
            >
              {title}
            </h2>
            {subtitle ? (
              <p className="mt-1 text-[12px] text-fg-secondary dark:text-fg-secondary-dark">
                {subtitle}
              </p>
            ) : null}
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label="close-modal"
            className="shrink-0 rounded-[6px] p-1 text-fg-tertiary hover:bg-black/[0.04] hover:text-fg-primary dark:text-fg-tertiary-dark dark:hover:bg-white/[0.06] dark:hover:text-fg-primary-dark"
          >
            <X size={14} aria-hidden="true" />
          </button>
        </header>

        <div className="min-h-0 flex-1 overflow-auto">{children}</div>

        {footer ? (
          <footer className="flex shrink-0 items-center justify-end gap-3 border-t border-hairline px-5 py-3 dark:border-hairline-dark">
            {footer}
          </footer>
        ) : null}
      </div>
    </div>
  );
}

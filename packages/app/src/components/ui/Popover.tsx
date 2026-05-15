import type { LucideIcon } from 'lucide-react';
import { useEffect, useId, useRef, useState, type ReactNode } from 'react';

export interface PopoverProps {
  /** The element the popover anchors to. Receives hover/focus listeners. */
  children: ReactNode;
  icon: LucideIcon;
  /** Short heading shown above the description. Optional. */
  title?: string;
  description: string;
  /** Optional "Learn more →" link rendered below the description. */
  docsUrl?: string;
  docsLabel?: string;
  testid?: string;
  /** Tailwind alignment for the floating panel. Default: right-aligned. */
  align?: 'left' | 'right';
}

/** Lightweight hover/focus popover — no Radix, no Floating UI.
 *  Trigger receives keyboard focus (role="button", tabIndex=0) and the
 *  panel announces via `role="tooltip"`. ESC closes it; clicking the
 *  trigger toggles for touch-friendly interaction. */
export function Popover({
  children,
  icon: Icon,
  title,
  description,
  docsUrl,
  docsLabel = 'Learn more →',
  testid,
  align = 'right',
}: PopoverProps): JSX.Element {
  const [open, setOpen] = useState(false);
  const descId = useId();
  const wrapRef = useRef<HTMLSpanElement | null>(null);

  useEffect(() => {
    if (!open) return undefined;
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === 'Escape') setOpen(false);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [open]);

  const alignClass = align === 'right' ? 'right-0' : 'left-0';

  return (
    <span
      ref={wrapRef}
      className="relative inline-flex"
      onMouseEnter={() => setOpen(true)}
      onMouseLeave={() => setOpen(false)}
      data-testid={testid}
    >
      <span
        role="button"
        tabIndex={0}
        aria-describedby={descId}
        aria-expanded={open}
        onFocus={() => setOpen(true)}
        onBlur={() => setOpen(false)}
        onClick={() => setOpen((v) => !v)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ' ') {
            e.preventDefault();
            setOpen((v) => !v);
          }
        }}
        className="inline-flex cursor-default outline-none focus-visible:ring-2 focus-visible:ring-brand focus-visible:ring-offset-1 focus-visible:ring-offset-canvas rounded-pill"
      >
        {children}
      </span>
      {open ? (
        <span
          id={descId}
          role="tooltip"
          className={`absolute top-full ${alignClass} z-30 mt-1.5 w-[260px] rounded-card border border-hairline bg-surface-elevated p-3 text-left shadow-modal dark:border-hairline-dark dark:bg-surface-elevated-dark dark:shadow-modal-dark`}
          onClick={(e) => e.stopPropagation()}
        >
          <span className="flex items-start gap-2">
            <Icon
              size={14}
              aria-hidden="true"
              className="mt-0.5 flex-shrink-0 text-brand"
              strokeWidth={2.2}
            />
            <span className="flex min-w-0 flex-col gap-1">
              {title ? (
                <span className="text-[12px] font-semibold tracking-tight text-fg-primary dark:text-fg-primary-dark">
                  {title}
                </span>
              ) : null}
              <span className="text-[11.5px] leading-snug text-fg-secondary dark:text-fg-secondary-dark">
                {description}
              </span>
              {docsUrl ? (
                <a
                  href={docsUrl}
                  target="_blank"
                  rel="noreferrer noopener"
                  className="mt-1 inline-flex w-fit text-[11px] font-medium text-brand hover:underline"
                >
                  {docsLabel}
                </a>
              ) : null}
            </span>
          </span>
        </span>
      ) : null}
    </span>
  );
}

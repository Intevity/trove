import type { ReactNode } from 'react';

export interface CardTitleProps {
  children: ReactNode;
  level?: 2 | 3;
  className?: string;
}

/** Tight, SF-style section title. 13px / semibold / -0.01em tracking —
 *  matches the AppHeader brand text scale rather than the older
 *  `text-sm` Bootstrap admin look. */
export function CardTitle({ children, level = 2, className = '' }: CardTitleProps): JSX.Element {
  const classes = `text-[13px] font-semibold tracking-tight text-fg-primary dark:text-fg-primary-dark ${className}`;
  if (level === 3) return <h3 className={classes}>{children}</h3>;
  return <h2 className={classes}>{children}</h2>;
}

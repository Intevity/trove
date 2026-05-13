import type { ReactNode } from 'react';

export interface CardHeaderProps {
  children: ReactNode;
  className?: string;
}

/** Consistent section header — title on the left, optional affordance
 *  on the right via the second child. Mirrors the existing
 *  `<header className="mb-2 flex items-center justify-between">`
 *  pattern across the dashboard. */
export function CardHeader({ children, className = '' }: CardHeaderProps): JSX.Element {
  return (
    <header className={`mb-2 flex items-center justify-between gap-2 ${className}`}>
      {children}
    </header>
  );
}

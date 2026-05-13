import type { ReactNode } from 'react';

export interface StatTileProps {
  /** The big numeral (or short string like "3s ago"). */
  value: ReactNode;
  /** The small uppercase label below the value. */
  label: string;
  /** Optional sub line below the label. Use `children` instead when the
   *  sub line needs nested elements (e.g. spans with their own
   *  data-testids). */
  sub?: ReactNode;
  /** Escape hatch for callers that need to preserve nested testid-bearing
   *  spans inside the sub line. Mutually-helpful with `sub` — both
   *  render if both are passed, sub above children. */
  children?: ReactNode;
  /** Forwarded to the root as `data-testid`. */
  testid?: string;
  className?: string;
}

/** KPI tile — large display numeral, uppercase caption label, optional
 *  sub line. Used by the Collector card on the Overview tab. The visual
 *  hierarchy is: numeral > label > sub. Numeral uses `tabular-nums` so
 *  digits stay aligned across adjacent tiles. */
export function StatTile({
  value,
  label,
  sub,
  children,
  testid,
  className = '',
}: StatTileProps): JSX.Element {
  const root = [
    'flex flex-col gap-1 rounded-tile border border-hairline bg-surface-elevated px-3 py-3',
    'dark:border-hairline-dark dark:bg-surface-elevated-dark',
    className,
  ]
    .filter(Boolean)
    .join(' ');
  return (
    <div className={root} data-testid={testid}>
      <span className="text-display tabular-nums text-fg-primary dark:text-fg-primary-dark truncate">
        {value}
      </span>
      <span className="text-caption uppercase text-fg-tertiary dark:text-fg-tertiary-dark">
        {label}
      </span>
      {sub ? (
        <span className="text-[11px] tabular-nums text-fg-secondary dark:text-fg-secondary-dark">
          {sub}
        </span>
      ) : null}
      {children ? (
        <span className="text-[11px] tabular-nums text-fg-secondary dark:text-fg-secondary-dark">
          {children}
        </span>
      ) : null}
    </div>
  );
}

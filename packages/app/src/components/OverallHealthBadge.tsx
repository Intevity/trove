import type { OverallHealth } from '@trove/shared';

import { overallHealthLabel } from '../lib/health.js';

const STYLES: Record<OverallHealth, string> = {
  green:
    'border-emerald-200 bg-emerald-50 text-emerald-900 dark:border-emerald-800 dark:bg-emerald-950 dark:text-emerald-200',
  amber:
    'border-amber-200 bg-amber-50 text-amber-900 dark:border-amber-800 dark:bg-amber-950 dark:text-amber-200',
  red: 'border-red-200 bg-red-50 text-red-900 dark:border-red-800 dark:bg-red-950 dark:text-red-200',
};

const DOT: Record<OverallHealth, string> = {
  green: 'bg-emerald-500',
  amber: 'bg-amber-500',
  red: 'bg-red-500',
};

interface Props {
  health: OverallHealth;
  /** Optional reason / detail line shown beneath the label. Used for
   *  cases like "metrics endpoint unreachable" so the user knows
   *  *why* the dashboard is amber rather than green. */
  detail?: string | undefined;
}

/** Mirror of the tray icon's colour in the dashboard header.
 *  PR 2's Rust `derive_overall_health` and PR 3's TS twin should
 *  produce the same value when fed the same inputs — the dev-hatch
 *  parity test in `dashboard-flow.spec.ts` confirms it visually. */
export function OverallHealthBadge({ health, detail }: Props): JSX.Element {
  return (
    <div
      data-testid="overall-health-badge"
      data-health={health}
      className={`flex items-center gap-3 rounded-md border px-3 py-2 text-sm ${STYLES[health]}`}
    >
      <span aria-hidden className={`inline-block h-3 w-3 rounded-full ${DOT[health]}`} />
      <span className="flex-1">
        <span className="font-medium">{overallHealthLabel(health)}</span>
        {detail ? <span className="ml-2 text-xs opacity-80">{detail}</span> : null}
      </span>
    </div>
  );
}

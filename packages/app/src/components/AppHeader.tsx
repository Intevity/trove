import type { OverallHealth } from '@trove/shared';

interface Props {
  health: OverallHealth;
}

const DOT_COLOR: Record<OverallHealth, string> = {
  green: 'bg-ios-green',
  amber: 'bg-ios-orange',
  red: 'bg-ios-red',
};

export function AppHeader({ health }: Props): JSX.Element {
  return (
    <header
      data-testid="app-header-bar"
      className="flex-shrink-0 flex items-center gap-3 px-4 pt-3 pb-2 border-b border-black/10 dark:border-white/10"
    >
      <div className="flex items-center gap-2 flex-shrink-0">
        <span className="relative flex h-2 w-2" aria-hidden>
          {health === 'green' && (
            <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-ios-green opacity-50" />
          )}
          <span className={`relative inline-flex rounded-full h-2 w-2 ${DOT_COLOR[health]}`} />
        </span>
        <span
          data-testid="app-header"
          className="text-[15px] font-semibold tracking-tight text-black dark:text-white"
        >
          Trove
        </span>
      </div>
    </header>
  );
}

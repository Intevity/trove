import { Activity, FileText, Settings, Zap } from 'lucide-react';
import { motion } from 'motion/react';

import type { OverallHealth } from '@trove/shared';

import troveLogo from '../assets/trove-logo.svg';
import { overallHealthLabel } from '../lib/health.js';

export type TabId = 'overview' | 'harnesses' | 'logs' | 'settings';

interface TabDef {
  id: TabId;
  label: string;
  icon: typeof Activity;
}

const TABS: readonly TabDef[] = [
  { id: 'overview', label: 'Overview', icon: Activity },
  { id: 'harnesses', label: 'Harnesses', icon: Zap },
  { id: 'logs', label: 'Logs', icon: FileText },
  { id: 'settings', label: 'Settings', icon: Settings },
];

const DOT_COLOR: Record<OverallHealth, string> = {
  green: 'bg-ios-green',
  amber: 'bg-ios-orange',
  red: 'bg-ios-red',
};

const DOT_TOOLTIP_SUFFIX: Record<OverallHealth, string> = {
  green: 'receiving telemetry',
  amber: 'collector or metrics endpoint not fully live',
  red: 'sidecar is not running',
};

interface Props {
  health: OverallHealth;
  /** Optional reason / detail line shown in the dot tooltip, e.g.
   *  "metrics endpoint unreachable". Sourced from Dashboard.badgeDetail(). */
  detail?: string | undefined;
  activeTab: TabId;
  onTabChange: (next: TabId) => void;
}

function dotTooltip(health: OverallHealth, detail: string | undefined): string {
  const label = overallHealthLabel(health);
  const reason = detail ?? DOT_TOOLTIP_SUFFIX[health];
  return reason ? `${label} — ${reason}` : label;
}

export function AppHeader({ health, detail, activeTab, onTabChange }: Props): JSX.Element {
  const tooltip = dotTooltip(health, detail);
  return (
    <header
      data-testid="app-header-bar"
      className="flex-shrink-0 flex items-center gap-2 px-3 pt-2 pb-2 border-b border-black/10 dark:border-white/10"
    >
      <div className="flex items-center gap-2 flex-shrink-0">
        <img
          src={troveLogo}
          alt=""
          aria-hidden="true"
          width={18}
          height={18}
          className="rounded-[4px]"
        />
        <span
          data-testid="app-header"
          className="text-[15px] font-semibold tracking-tight text-black dark:text-white"
        >
          Trove
        </span>
        <span
          data-testid="app-health-dot"
          data-health={health}
          aria-label={tooltip}
          title={tooltip}
          className="relative flex h-2 w-2 ml-0.5 cursor-help"
        >
          {health === 'green' && (
            <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-ios-green opacity-50" />
          )}
          <span className={`relative inline-flex rounded-full h-2 w-2 ${DOT_COLOR[health]}`} />
        </span>
      </div>

      <div
        role="tablist"
        className="ml-auto flex bg-black/[0.06] dark:bg-white/[0.08] rounded-xl p-[3px] min-w-0"
      >
        {TABS.map(({ id, label, icon: Icon }) => {
          const active = activeTab === id;
          return (
            <button
              key={id}
              type="button"
              role="tab"
              aria-selected={active}
              data-testid={`tab-${id}`}
              onClick={() => onTabChange(id)}
              className={`relative flex items-center justify-center gap-1 px-2 py-1 rounded-[9px] text-[11px] font-medium transition-colors duration-150 ${
                active
                  ? 'text-black dark:text-white'
                  : 'text-ios-gray hover:text-black dark:hover:text-white'
              }`}
            >
              {active && (
                <motion.span
                  layoutId="trove-tab-pill"
                  className="absolute inset-0 rounded-[9px] bg-white dark:bg-[#3A3A3C] shadow-[0_1px_3px_rgba(0,0,0,0.15)]"
                  transition={{ type: 'spring', stiffness: 500, damping: 40 }}
                />
              )}
              <span className="relative z-10 flex items-center gap-1 transform-gpu">
                <Icon size={11} strokeWidth={2.2} />
                {label}
              </span>
            </button>
          );
        })}
      </div>
    </header>
  );
}

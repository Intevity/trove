import { motion } from 'motion/react';
import { Activity, FileText, Settings, Zap } from 'lucide-react';

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

interface Props {
  activeTab: TabId;
  onChange: (next: TabId) => void;
}

export function TabNav({ activeTab, onChange }: Props): JSX.Element {
  return (
    <div className="flex-shrink-0 px-4 py-2">
      <div role="tablist" className="flex bg-black/[0.06] dark:bg-white/[0.08] rounded-xl p-[3px]">
        {TABS.map(({ id, label, icon: Icon }) => {
          const active = activeTab === id;
          return (
            <button
              key={id}
              type="button"
              role="tab"
              aria-selected={active}
              data-testid={`tab-${id}`}
              onClick={() => onChange(id)}
              className={`relative flex-1 flex items-center justify-center gap-1.5 px-2 py-1.5 rounded-[9px] text-[12px] font-medium transition-colors duration-150 ${
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
              <span className="relative z-10 flex items-center gap-1.5 transform-gpu">
                <Icon size={12} strokeWidth={2.2} />
                {label}
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}

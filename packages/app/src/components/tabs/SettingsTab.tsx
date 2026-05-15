import type { LucideIcon } from 'lucide-react';
import { Info, RefreshCw, ShieldCheck } from 'lucide-react';
import type { ReactNode } from 'react';

import type { AppState } from '@trove/shared';

import troveLogo from '../../assets/trove-logo.svg';
import { setAutoUpdateEnabled } from '../../lib/ipc.js';
import { AutoUpdate } from '../Settings/AutoUpdate.js';
import { IdentityPanel } from '../Settings/IdentityPanel.js';
import { Card } from '../ui/index.js';

interface Props {
  appState: AppState;
  onAppStateRefresh: () => void | Promise<void>;
}

export function SettingsTab({ appState, onAppStateRefresh }: Props): JSX.Element {
  return (
    <div className="mx-auto flex max-w-2xl flex-col gap-6 px-5 py-5">
      <header className="flex flex-col gap-1">
        <h1 className="text-[22px] font-semibold tracking-tight text-fg-primary dark:text-fg-primary-dark">
          Settings
        </h1>
        <p className="text-[12px] text-fg-tertiary dark:text-fg-tertiary-dark">
          Configure how Trove updates and what it tags onto outgoing telemetry. Additional
          preferences will land here as the app grows.
        </p>
      </header>

      <SettingsSection icon={RefreshCw} label="General">
        <AutoUpdate
          enabled={appState.autoUpdateEnabled}
          onToggle={async (next) => {
            await setAutoUpdateEnabled(next);
            await onAppStateRefresh();
          }}
        />
      </SettingsSection>

      <SettingsSection icon={ShieldCheck} label="Privacy & Identity">
        <IdentityPanel
          enabled={appState.identity.enabled}
          manualName={appState.identity.name}
          manualEmail={appState.identity.email}
          onChanged={() => void onAppStateRefresh()}
        />
      </SettingsSection>

      <SettingsSection icon={Info} label="About">
        <AboutCard />
      </SettingsSection>
    </div>
  );
}

interface SettingsSectionProps {
  icon: LucideIcon;
  label: string;
  children: ReactNode;
}

/** Section header pattern — a small uppercase brand-tinted caption sits
 *  above its card. Lets the page grow to additional sections without
 *  losing the visual rhythm. */
function SettingsSection({ icon: Icon, label, children }: SettingsSectionProps): JSX.Element {
  return (
    <section className="flex flex-col gap-2">
      <div className="flex items-center gap-1.5 px-1">
        <Icon size={11} className="text-brand" strokeWidth={2.4} aria-hidden="true" />
        <h2 className="text-[10px] font-semibold uppercase tracking-[0.08em] text-fg-tertiary dark:text-fg-tertiary-dark">
          {label}
        </h2>
      </div>
      {children}
    </section>
  );
}

function AboutCard(): JSX.Element {
  const displayVersion = __APP_VERSION__.startsWith('v')
    ? __APP_VERSION__
    : __APP_VERSION__ === 'dev'
      ? 'dev'
      : `v${__APP_VERSION__}`;
  return (
    <Card padding="md" testid="settings-about">
      <div className="flex items-center gap-4">
        <img
          src={troveLogo}
          alt=""
          aria-hidden="true"
          width={44}
          height={44}
          className="flex-shrink-0"
        />
        <div className="flex min-w-0 flex-col">
          <p className="text-[15px] font-semibold tracking-tight text-fg-primary dark:text-fg-primary-dark">
            Trove
          </p>
          <p className="text-[12px] text-fg-secondary dark:text-fg-secondary-dark">
            Local-first telemetry for AI coding harnesses.
          </p>
          <p className="mt-1 text-[11px] tabular-nums text-fg-tertiary dark:text-fg-tertiary-dark">
            {displayVersion} · Built by Intevity
          </p>
        </div>
      </div>
    </Card>
  );
}

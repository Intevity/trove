import type { AppState } from '@trove/shared';

import { setAutoUpdateEnabled } from '../../lib/ipc.js';
import { AutoUpdate } from '../Settings/AutoUpdate.js';
import { IdentityPanel } from '../Settings/IdentityPanel.js';

interface Props {
  appState: AppState;
  onAppStateRefresh: () => void | Promise<void>;
}

export function SettingsTab({ appState, onAppStateRefresh }: Props): JSX.Element {
  return (
    <div className="flex flex-col gap-4 px-4 py-3">
      <AutoUpdate
        enabled={appState.autoUpdateEnabled}
        onToggle={async (next) => {
          await setAutoUpdateEnabled(next);
          await onAppStateRefresh();
        }}
      />

      <IdentityPanel
        enabled={appState.identity.enabled}
        manualName={appState.identity.name}
        manualEmail={appState.identity.email}
        onChanged={() => void onAppStateRefresh()}
      />
    </div>
  );
}

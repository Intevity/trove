import { LogOut, Trash2 } from 'lucide-react';
import { useState } from 'react';

import { quitApp, uninstallApp } from '../../lib/ipc.js';
import { Button, Card, Sheet } from '../ui/index.js';

type RemoveDataChoice = 'keep' | 'remove';

/** Settings card surfacing the two app-lifecycle actions:
 *  - **Quit Trove** — clean exit (mirrors the tray menu).
 *  - **Uninstall** — opens a confirmation modal that lets the user
 *    decide whether to keep or wipe persisted data, then schedules a
 *    detached helper to remove the installed bundle. */
export function ApplicationActions(): JSX.Element {
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [choice, setChoice] = useState<RemoveDataChoice>('keep');
  const [busy, setBusy] = useState<'quit' | 'uninstall' | null>(null);

  const onQuit = async (): Promise<void> => {
    setBusy('quit');
    try {
      await quitApp();
    } catch {
      setBusy(null);
    }
  };

  const onConfirmUninstall = async (): Promise<void> => {
    setBusy('uninstall');
    try {
      await uninstallApp(choice === 'remove');
    } catch {
      // If the IPC errored, the app didn't exit — re-enable the modal.
      setBusy(null);
    }
  };

  return (
    <>
      <Card padding="md" testid="settings-application">
        <div className="flex flex-col divide-y divide-hairline dark:divide-hairline-dark">
          <Row
            title="Quit Trove"
            subtitle="Exit the app and stop the bundled collector."
            action={
              <Button
                variant="secondary"
                size="sm"
                onClick={() => void onQuit()}
                disabled={busy !== null}
                testid="settings-quit-trove"
                aria-label="quit-trove"
              >
                <LogOut size={12} aria-hidden="true" />
                Quit
              </Button>
            }
          />
          <Row
            title="Uninstall Trove"
            subtitle="Remove the app from this Mac. You'll be asked whether to keep your data."
            action={
              <Button
                variant="destructive"
                size="sm"
                onClick={() => setConfirmOpen(true)}
                disabled={busy !== null}
                testid="settings-uninstall-trove"
                aria-label="uninstall-trove"
              >
                <Trash2 size={12} aria-hidden="true" />
                Uninstall…
              </Button>
            }
          />
        </div>
      </Card>

      <Sheet
        open={confirmOpen}
        onClose={() => (busy === null ? setConfirmOpen(false) : undefined)}
        size="md"
        title="Uninstall Trove?"
        subtitle="Trove will exit and remove itself from this Mac once you confirm."
        testid="uninstall-confirm"
        footer={
          <>
            <Button
              variant="secondary"
              size="md"
              onClick={() => setConfirmOpen(false)}
              disabled={busy === 'uninstall'}
            >
              Cancel
            </Button>
            <Button
              variant="destructive"
              size="md"
              onClick={() => void onConfirmUninstall()}
              loading={busy === 'uninstall'}
              testid="uninstall-confirm-submit"
            >
              {busy === 'uninstall' ? 'Uninstalling…' : 'Uninstall'}
            </Button>
          </>
        }
      >
        <div className="space-y-4 px-5 py-4">
          <p className="text-[13px] text-fg-secondary dark:text-fg-secondary-dark">
            Choose what to do with the data Trove stores on this machine — your saved backends,
            mapping customizations, identity overrides, and the local collector log.
          </p>
          <fieldset className="flex flex-col gap-2.5">
            <legend className="sr-only">What to do with your data</legend>
            <ChoiceRow
              checked={choice === 'keep'}
              onChange={() => setChoice('keep')}
              label="Keep my data"
              description="Leaves Trove's config, secrets, and logs on disk so re-installing later picks up where you left off."
            />
            <ChoiceRow
              checked={choice === 'remove'}
              onChange={() => setChoice('remove')}
              label="Remove everything"
              description="Deletes Trove's data directories and any backend credentials it stored. This cannot be undone."
            />
          </fieldset>
        </div>
      </Sheet>
    </>
  );
}

interface RowProps {
  title: string;
  subtitle: string;
  action: JSX.Element;
}

function Row({ title, subtitle, action }: RowProps): JSX.Element {
  return (
    <div className="flex items-center justify-between gap-4 py-3 first:pt-0 last:pb-0">
      <div className="min-w-0">
        <p className="text-[13px] font-medium text-fg-primary dark:text-fg-primary-dark">{title}</p>
        <p className="mt-0.5 text-[12px] text-fg-secondary dark:text-fg-secondary-dark">
          {subtitle}
        </p>
      </div>
      {action}
    </div>
  );
}

interface ChoiceRowProps {
  checked: boolean;
  onChange: () => void;
  label: string;
  description: string;
}

function ChoiceRow({ checked, onChange, label, description }: ChoiceRowProps): JSX.Element {
  return (
    <label
      className={[
        'flex cursor-pointer items-start gap-2.5 rounded-card border px-3 py-2.5 transition-colors',
        checked
          ? 'border-brand bg-brand/[0.06] dark:bg-brand/[0.10]'
          : 'border-hairline bg-surface hover:bg-surface-elevated dark:border-hairline-dark dark:bg-surface-dark dark:hover:bg-surface-elevated-dark',
      ].join(' ')}
    >
      <input
        type="radio"
        name="uninstall-data-choice"
        checked={checked}
        onChange={onChange}
        className="mt-1 h-3.5 w-3.5 accent-brand"
      />
      <span className="flex min-w-0 flex-col">
        <span className="text-[13px] font-medium text-fg-primary dark:text-fg-primary-dark">
          {label}
        </span>
        <span className="text-[12px] text-fg-secondary dark:text-fg-secondary-dark">
          {description}
        </span>
      </span>
    </label>
  );
}

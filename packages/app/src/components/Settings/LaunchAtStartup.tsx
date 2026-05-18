import { Rocket } from 'lucide-react';

import { Card, CardHeader, CardTitle } from '../ui/index.js';

interface Props {
  enabled: boolean;
  onToggle: (next: boolean) => void | Promise<void>;
}

export function LaunchAtStartup({ enabled, onToggle }: Props): JSX.Element {
  function handleToggle(e: React.ChangeEvent<HTMLInputElement>): void {
    void onToggle(e.target.checked);
  }

  return (
    <Card testid="launch-at-startup-panel">
      <CardHeader>
        <div className="flex items-center gap-2">
          <span className="flex h-7 w-7 items-center justify-center rounded-[8px] bg-brand/[0.14] text-brand">
            <Rocket size={14} strokeWidth={2.2} aria-hidden="true" />
          </span>
          <div className="flex flex-col">
            <CardTitle>Launch at startup</CardTitle>
            <span className="text-[11px] text-fg-tertiary dark:text-fg-tertiary-dark">
              Trove starts automatically when you log in.
            </span>
          </div>
        </div>
      </CardHeader>

      <label className="flex items-start gap-3 text-[13px] text-fg-secondary dark:text-fg-secondary-dark">
        <input
          type="checkbox"
          data-testid="launch-at-startup-toggle"
          checked={enabled}
          onChange={handleToggle}
          className="mt-0.5 h-4 w-4 rounded border-fg-tertiary focus:ring-brand dark:border-fg-tertiary-dark"
        />
        <span className="flex flex-col">
          <span className="text-fg-primary dark:text-fg-primary-dark">
            Open Trove when I sign in
          </span>
          <span className="text-[12px] text-fg-tertiary dark:text-fg-tertiary-dark">
            On by default so harness telemetry is captured from the moment you sit down. Turn off if
            you'd rather start Trove manually.
          </span>
        </span>
      </label>
    </Card>
  );
}

import { Download } from 'lucide-react';
import { useState } from 'react';

import type { UpdateMetadata } from '@trove/shared';

import { TroveIpcError, checkForUpdates } from '../../lib/ipc.js';
import { Button, Card, CardHeader, CardTitle } from '../ui/index.js';

interface Props {
  enabled: boolean;
  onToggle: (next: boolean) => void | Promise<void>;
}

export function AutoUpdate({ enabled, onToggle }: Props): JSX.Element {
  const [checking, setChecking] = useState(false);
  const [result, setResult] = useState<UpdateMetadata | null>(null);
  const [error, setError] = useState<TroveIpcError | null>(null);

  async function handleCheck(): Promise<void> {
    setChecking(true);
    setError(null);
    setResult(null);
    try {
      const r = await checkForUpdates();
      setResult(r);
    } catch (err) {
      if (err instanceof TroveIpcError) setError(err);
    } finally {
      setChecking(false);
    }
  }

  function handleToggle(e: React.ChangeEvent<HTMLInputElement>): void {
    void onToggle(e.target.checked);
  }

  return (
    <Card testid="auto-update-panel">
      <CardHeader>
        <div className="flex items-center gap-2">
          <span className="flex h-7 w-7 items-center justify-center rounded-[8px] bg-brand/[0.14] text-brand">
            <Download size={14} strokeWidth={2.2} aria-hidden="true" />
          </span>
          <div className="flex flex-col">
            <CardTitle>Updates</CardTitle>
            <span className="text-[11px] text-fg-tertiary dark:text-fg-tertiary-dark">
              Trove checks its secure update channel for new versions.
            </span>
          </div>
        </div>
      </CardHeader>

      <label className="flex items-start gap-3 text-[13px] text-fg-secondary dark:text-fg-secondary-dark">
        <input
          type="checkbox"
          data-testid="auto-update-toggle"
          checked={enabled}
          onChange={handleToggle}
          className="mt-0.5 h-4 w-4 rounded border-fg-tertiary focus:ring-brand dark:border-fg-tertiary-dark"
        />
        <span className="flex flex-col">
          <span className="text-fg-primary dark:text-fg-primary-dark">
            Automatically check for updates
          </span>
          <span className="text-[12px] text-fg-tertiary dark:text-fg-tertiary-dark">
            Off by default. Trove only checks for updates every four hours when this is on, or when
            you click the button below.
          </span>
        </span>
      </label>

      <div className="mt-3 flex items-center gap-3">
        <Button
          variant="secondary"
          size="sm"
          testid="auto-update-check-now"
          onClick={() => void handleCheck()}
          disabled={checking}
        >
          {checking ? 'Checking…' : 'Check for updates now…'}
        </Button>
        <span
          data-testid="auto-update-status"
          className="text-[12px] text-fg-secondary dark:text-fg-secondary-dark"
        >
          {renderStatus(result, error)}
        </span>
      </div>
    </Card>
  );
}

function renderStatus(result: UpdateMetadata | null, error: TroveIpcError | null): string {
  if (error) {
    if (error.cause.kind === 'updater-check-failed') {
      return `Check failed: ${error.cause.reason}`;
    }
    return `Check failed: ${error.cause.kind}`;
  }
  if (!result) return '';
  if (result.available && result.version) {
    return `Update available: v${result.version} (running v${result.current}).`;
  }
  return `You're on v${result.current} — up to date.`;
}

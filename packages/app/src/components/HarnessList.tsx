import type { DetectedHarness, HarnessId } from '@trove/shared';

const HARNESS_LABELS: Record<HarnessId, string> = {
  'claude-code': 'Claude Code',
  'gemini-cli': 'Gemini CLI',
  'codex-cli': 'OpenAI Codex CLI',
  'qwen-code': 'Qwen Code',
  opencode: 'OpenCode',
  'cursor-ide': 'Cursor IDE',
  'cursor-cli': 'Cursor CLI',
  cline: 'Cline',
  aider: 'Aider',
  'copilot-cli': 'GitHub Copilot CLI',
};

/** Per-harness coverage advisory surfaced as a badge next to the
 *  telemetry status. Currently only cursor-cli carries one — Cursor's
 *  CLI fires a strict subset of the events the IDE does, so users
 *  enabling Cursor CLI alone capture less than they would with the
 *  IDE. The badge text stays short (the row is tight); the optional
 *  `tooltip` field renders as a `title` attribute so hovering surfaces
 *  the longer explanation without taking row real-estate. The
 *  `docsUrl` link points at the Cursor hooks documentation where the
 *  coverage gap is described upstream. */
interface CoverageNote {
  text: string;
  tooltip: string;
  docsUrl: string;
}
const COVERAGE_NOTES: Partial<Record<HarnessId, CoverageNote>> = {
  'cursor-cli': {
    text: 'Partial event coverage',
    tooltip:
      "Cursor CLI (cursor-agent) fires only a subset of Cursor's hook events — primarily beforeShellExecution and afterShellExecution. Cursor IDE fires the full surface. See Cursor's hooks docs.",
    docsUrl: 'https://cursor.com/docs/hooks',
  },
};

export interface HarnessListProps {
  harnesses: DetectedHarness[];
  loading: boolean;
  /** Called when the user clicks "Enable" — the parent opens the
   *  PatchPreviewModal for `id`. */
  onEnable?: (id: HarnessId) => void;
  /** Called when the user clicks "Disable" — the parent calls
   *  `revertPatch` for `id`. */
  onDisable?: (id: HarnessId) => void;
  /** Set of harness IDs whose row is currently mid-revert (button
   *  disables + label changes). */
  busyIds?: ReadonlySet<HarnessId>;
}

export function HarnessList({
  harnesses,
  loading,
  onEnable,
  onDisable,
  busyIds,
}: HarnessListProps): JSX.Element {
  if (loading) {
    return (
      <p className="text-sm text-slate-500 dark:text-slate-400" data-testid="harness-list-loading">
        Detecting harnesses…
      </p>
    );
  }

  if (harnesses.length === 0) {
    return (
      <p className="text-sm text-slate-500 dark:text-slate-400" data-testid="harness-list-empty">
        No supported harnesses detected on this machine.
      </p>
    );
  }

  return (
    <ul
      className="divide-y divide-slate-200 rounded-lg border border-slate-200 dark:divide-slate-800 dark:border-slate-800"
      data-testid="harness-list"
    >
      {harnesses.map((harness) => (
        <HarnessRow
          key={harness.id}
          harness={harness}
          onEnable={onEnable}
          onDisable={onDisable}
          busy={busyIds?.has(harness.id) ?? false}
        />
      ))}
    </ul>
  );
}

interface HarnessRowProps {
  harness: DetectedHarness;
  onEnable: ((id: HarnessId) => void) | undefined;
  onDisable: ((id: HarnessId) => void) | undefined;
  busy: boolean;
}

function HarnessRow({ harness, onEnable, onDisable, busy }: HarnessRowProps): JSX.Element {
  const adapterAvailable = harness.adapterAvailable;
  const detectionLabel = describeDetection(harness);
  const telemetryLabel = describeTelemetry(harness);
  const enabled = harness.troveRegionPresent;

  const buttonLabel = busy
    ? enabled
      ? 'Disabling…'
      : 'Enabling…'
    : !adapterAvailable
      ? 'Adapter not yet available'
      : enabled
        ? 'Disable'
        : 'Enable';

  const handleClick = (): void => {
    if (!adapterAvailable || busy) return;
    if (enabled) {
      onDisable?.(harness.id);
    } else {
      onEnable?.(harness.id);
    }
  };

  return (
    <li
      className="flex items-center justify-between gap-4 px-4 py-3"
      data-testid={`harness-row-${harness.id}`}
    >
      <div>
        <p className="text-sm font-medium text-slate-900 dark:text-slate-100">
          {HARNESS_LABELS[harness.id]}
        </p>
        <p className="text-xs text-slate-500 dark:text-slate-400">{detectionLabel}</p>
      </div>
      <div className="flex flex-col items-end gap-1 text-right">
        <span
          className="text-xs uppercase tracking-wide text-slate-500 dark:text-slate-400"
          data-testid={`harness-telemetry-${harness.id}`}
        >
          {telemetryLabel}
        </span>
        {COVERAGE_NOTES[harness.id] ? (
          <a
            className="text-xs italic text-amber-600 underline-offset-2 hover:underline dark:text-amber-400"
            data-testid={`harness-coverage-note-${harness.id}`}
            href={COVERAGE_NOTES[harness.id]!.docsUrl}
            target="_blank"
            rel="noreferrer noopener"
            title={COVERAGE_NOTES[harness.id]!.tooltip}
          >
            {COVERAGE_NOTES[harness.id]!.text}
          </a>
        ) : null}
        <button
          type="button"
          onClick={handleClick}
          disabled={!harness.detected || !adapterAvailable || busy}
          className="rounded-md border border-slate-300 bg-white px-3 py-1 text-xs font-medium text-slate-900 shadow-sm transition disabled:cursor-not-allowed disabled:opacity-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-100"
          aria-label={`toggle-${harness.id}`}
        >
          {buttonLabel}
        </button>
      </div>
    </li>
  );
}

function describeDetection(harness: DetectedHarness): string {
  if (!harness.detected) {
    return 'Not detected on this machine';
  }
  switch (harness.detectionMethod) {
    case 'config-dir':
      return harness.configPath
        ? `Detected via config — ${harness.configPath}`
        : 'Detected via config dir';
    case 'path-binary':
      return 'Detected on PATH';
    case 'app-bundle':
      return 'Detected as app bundle';
    case null:
    default:
      return 'Detected';
  }
}

function describeTelemetry(harness: DetectedHarness): string {
  switch (harness.telemetry) {
    case 'on':
      return 'Telemetry on';
    case 'off':
      return 'Telemetry off';
    case 'unknown':
    default:
      return 'Telemetry unknown';
  }
}

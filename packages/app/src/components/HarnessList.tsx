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

/** Adapters available in Sprint 3 — only Claude Code (PR 2) and
 *  Gemini CLI (PR 3) ship within this sprint. The remaining Tier 1
 *  rows are detected and listed but their toggles stay disabled until
 *  Sprint 4 adds the codex-cli + qwen-code adapters. */
const ADAPTERS_AVAILABLE_IN_SPRINT_3: HarnessId[] = ['claude-code', 'gemini-cli'];

export interface HarnessListProps {
  harnesses: DetectedHarness[];
  loading: boolean;
}

export function HarnessList({ harnesses, loading }: HarnessListProps): JSX.Element {
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
        <HarnessRow key={harness.id} harness={harness} />
      ))}
    </ul>
  );
}

interface HarnessRowProps {
  harness: DetectedHarness;
}

function HarnessRow({ harness }: HarnessRowProps): JSX.Element {
  const adapterAvailable = ADAPTERS_AVAILABLE_IN_SPRINT_3.includes(harness.id);
  const detectionLabel = describeDetection(harness);
  const telemetryLabel = describeTelemetry(harness);

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
        <button
          type="button"
          disabled={!harness.detected || !adapterAvailable}
          className="rounded-md border border-slate-300 bg-white px-3 py-1 text-xs font-medium text-slate-900 shadow-sm transition disabled:cursor-not-allowed disabled:opacity-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-100"
          aria-label={`toggle-${harness.id}`}
        >
          {adapterAvailable ? 'Enable' : 'Adapter coming in Sprint 4'}
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

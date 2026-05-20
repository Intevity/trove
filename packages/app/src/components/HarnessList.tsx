import type { LucideIcon } from 'lucide-react';
import { Info, Search, Sparkles, X } from 'lucide-react';
import { useMemo, useState } from 'react';

import type { DetectedHarness, HarnessId } from '@trove/shared';

import { HARNESS_LABELS, HarnessLogo } from '../lib/logos.js';
import { HARNESS_SETUP_GUIDES, HarnessSetupGuideModal } from './HarnessSetupGuide.js';
import {
  Button,
  Card,
  CardHeader,
  CardTitle,
  Pill,
  Popover,
  StatusDot,
  type DotStatus,
  type PillTone,
} from './ui/index.js';

interface HarnessBadge {
  /** Short pill copy. */
  label: string;
  tone: PillTone;
  icon: LucideIcon;
  /** Long-form copy revealed in the popover when the user hovers / focuses. */
  description: string;
  /** Optional "Learn more →" link in the popover. */
  docsUrl?: string;
}

/** Coverage / detection nuance surfaced as a hoverable pill next to the
 *  telemetry status. Replaces the prior `COVERAGE_NOTES` orange link and
 *  the italic `TELEMETRY_HINTS` paragraph — both wanted one line of
 *  real estate, neither was visually consistent with the rest of the
 *  row. */
const HARNESS_BADGES: Partial<Record<HarnessId, HarnessBadge>> = {
  'cursor-cli': {
    label: 'Partial Coverage',
    tone: 'amber',
    icon: Info,
    description:
      "Cursor CLI (cursor-agent) fires only a subset of Cursor's hook events — primarily beforeShellExecution and afterShellExecution. Cursor IDE fires the full surface.",
    docsUrl: 'https://cursor.com/docs/hooks',
  },
  cline: {
    label: 'Best Effort',
    tone: 'amber',
    icon: Info,
    description:
      "Cline doesn't emit OpenTelemetry natively. Trove watches Cline's per-task globalStorage records and emits OTLP logs derived from them. Token counts and durations are captured; raw conversation content stays on disk unless prompt logging is explicitly enabled.",
    docsUrl: 'https://github.com/cline/cline',
  },
  aider: {
    label: 'Best Effort',
    tone: 'amber',
    icon: Info,
    description:
      "Aider doesn't emit OpenTelemetry natively. Trove installs a shell-rc wrapper that runs the real aider and tees its session log; a watcher parses the log into OTLP records. Open a fresh terminal after enabling so the new shell function takes effect.",
    docsUrl: 'https://aider.chat/docs/',
  },
  'copilot-cli': {
    label: 'Best Effort',
    tone: 'amber',
    icon: Info,
    description:
      "GitHub Copilot CLI doesn't emit OpenTelemetry natively. Trove installs shell-rc wrappers exposed as `copilot` and `gh-copilot` that run the real Copilot CLI and log invocation counts + durations. The wrapper prefers the new standalone `copilot` binary; if it isn't installed it falls back to the deprecated `gh copilot` gh-extension.",
    docsUrl: 'https://docs.github.com/en/copilot/github-copilot-in-the-cli',
  },
  'claude-desktop': {
    label: 'Auto-detected',
    tone: 'brand',
    icon: Sparkles,
    description: "Detected in-stream from each Cowork session's local audit log; no setup needed.",
  },
};

export interface HarnessListProps {
  harnesses: DetectedHarness[];
  loading: boolean;
  onEnable?: (id: HarnessId) => void;
  onDisable?: (id: HarnessId) => void;
  busyIds?: ReadonlySet<HarnessId>;
  onRefresh?: () => void | Promise<void>;
  /** Source-of-truth for which harnesses are currently active. Comes
   *  from `AppState.harnesses.filter(h => h.enabled)` upstream — using
   *  the persisted config rather than a filesystem probe keeps the
   *  toggle in sync after add/remove cycles and correctly distinguishes
   *  harnesses that share a config file (e.g. Cursor IDE and Cursor
   *  CLI both live in `~/.cursor/hooks.json`). Defaults to empty so
   *  unit tests rendering the list without a Dashboard don't have to
   *  thread the prop. */
  enabledIds?: ReadonlySet<HarnessId>;
}

export function HarnessList({
  harnesses,
  loading,
  onEnable,
  onDisable,
  busyIds,
  onRefresh,
  enabledIds,
}: HarnessListProps): JSX.Element {
  const [query, setQuery] = useState('');
  const [openGuideFor, setOpenGuideFor] = useState<HarnessId | null>(null);

  const sorted = useMemo(() => {
    const q = query.trim().toLowerCase();
    const matches = q
      ? harnesses.filter(
          (h) => HARNESS_LABELS[h.id].toLowerCase().includes(q) || h.id.toLowerCase().includes(q),
        )
      : harnesses;
    const detected: DetectedHarness[] = [];
    const undetected: DetectedHarness[] = [];
    for (const h of matches) {
      (h.detected ? detected : undetected).push(h);
    }
    return [...detected, ...undetected];
  }, [harnesses, query]);

  const hasQuery = query.trim().length > 0;
  const showSearch = !loading && harnesses.length > 0;

  return (
    <Card as="section" padding="sm" testid="harness-list-section">
      <CardHeader>
        <CardTitle>Detected harnesses</CardTitle>
        {onRefresh ? (
          <Button
            variant="secondary"
            size="sm"
            testid="harness-list-refresh"
            disabled={loading}
            onClick={() => void onRefresh()}
            aria-label="refresh harness detection"
            title="Re-run detection; useful when you've just installed a harness"
          >
            <svg
              width="11"
              height="11"
              viewBox="0 0 16 16"
              aria-hidden="true"
              className={loading ? 'animate-spin' : ''}
            >
              <path
                fill="none"
                stroke="currentColor"
                strokeWidth="1.6"
                strokeLinecap="round"
                strokeLinejoin="round"
                d="M3 8a5 5 0 1 1 1.464 3.536M3 13v-2.5h2.5"
              />
            </svg>
            {loading ? 'Refreshing…' : 'Refresh'}
          </Button>
        ) : null}
      </CardHeader>

      {showSearch ? (
        <div className="relative mb-2">
          <Search
            size={13}
            aria-hidden="true"
            className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-fg-tertiary dark:text-fg-tertiary-dark"
          />
          <input
            type="text"
            role="searchbox"
            data-testid="harness-search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Filter harnesses by name…"
            aria-label="Filter harnesses by name"
            className="w-full rounded-[8px] border border-hairline bg-surface-elevated py-1.5 pl-8 pr-8 text-[12px] text-fg-primary placeholder:text-fg-tertiary focus:border-brand focus:outline-none focus:ring-1 focus:ring-brand dark:border-hairline-dark dark:bg-surface-elevated-dark dark:text-fg-primary-dark dark:placeholder:text-fg-tertiary-dark"
          />
          {hasQuery ? (
            <button
              type="button"
              data-testid="harness-search-clear"
              aria-label="Clear search"
              onClick={() => setQuery('')}
              className="absolute right-2 top-1/2 -translate-y-1/2 rounded p-0.5 text-fg-tertiary hover:text-fg-primary dark:text-fg-tertiary-dark dark:hover:text-fg-primary-dark"
            >
              <X size={13} aria-hidden="true" />
            </button>
          ) : null}
        </div>
      ) : null}

      {loading ? (
        <p
          className="text-[13px] text-fg-secondary dark:text-fg-secondary-dark"
          data-testid="harness-list-loading"
        >
          Detecting harnesses…
        </p>
      ) : harnesses.length === 0 ? (
        <p
          className="text-[13px] text-fg-secondary dark:text-fg-secondary-dark"
          data-testid="harness-list-empty"
        >
          No supported harnesses detected on this machine.
        </p>
      ) : sorted.length === 0 ? (
        <p
          className="text-[13px] text-fg-secondary dark:text-fg-secondary-dark"
          data-testid="harness-list-no-matches"
        >
          No harnesses match “{query.trim()}”.
        </p>
      ) : (
        <ul
          className="divide-y divide-hairline overflow-hidden rounded-card border border-hairline dark:divide-hairline-dark dark:border-hairline-dark"
          data-testid="harness-list"
        >
          {sorted.map((harness) => (
            <HarnessRow
              key={harness.id}
              harness={harness}
              onEnable={onEnable}
              onDisable={onDisable}
              busy={busyIds?.has(harness.id) ?? false}
              enabled={enabledIds?.has(harness.id) ?? false}
              onOpenGuide={() => setOpenGuideFor(harness.id)}
            />
          ))}
        </ul>
      )}
      {openGuideFor && HARNESS_SETUP_GUIDES[openGuideFor] ? (
        <HarnessSetupGuideModal
          guide={HARNESS_SETUP_GUIDES[openGuideFor]!}
          open
          onClose={() => setOpenGuideFor(null)}
          testid={`harness-setup-guide-${openGuideFor}`}
        />
      ) : null}
    </Card>
  );
}

interface HarnessRowProps {
  harness: DetectedHarness;
  onEnable: ((id: HarnessId) => void) | undefined;
  onDisable: ((id: HarnessId) => void) | undefined;
  busy: boolean;
  /** True when this harness has an `enabled: true` entry in
   *  `AppState.harnesses`. Drives the Enable/Disable button label
   *  and click handler. */
  enabled: boolean;
  onOpenGuide: () => void;
}

function HarnessRow({
  harness,
  onEnable,
  onDisable,
  busy,
  enabled,
  onOpenGuide,
}: HarnessRowProps): JSX.Element {
  const adapterAvailable = harness.adapterAvailable;
  const guide = HARNESS_SETUP_GUIDES[harness.id];
  const hasGuide = Boolean(guide);
  const detectionLabel = describeDetection(harness);
  const telemetryLabel = describeTelemetry(harness);
  const badge = HARNESS_BADGES[harness.id];

  const buttonLabel = busy
    ? enabled
      ? 'Disabling…'
      : 'Enabling…'
    : !adapterAvailable
      ? hasGuide
        ? (guide!.buttonLabel ?? 'Set up →')
        : 'Adapter not yet available'
      : enabled
        ? 'Disable'
        : 'Enable';

  const handleClick = (): void => {
    if (busy) return;
    if (!adapterAvailable) {
      if (hasGuide) onOpenGuide();
      return;
    }
    if (enabled) {
      onDisable?.(harness.id);
    } else {
      onEnable?.(harness.id);
    }
  };

  const rowClass = harness.detected
    ? 'flex items-center justify-between gap-4 bg-surface px-4 py-3 dark:bg-surface-dark'
    : 'flex items-center justify-between gap-4 bg-canvas/40 px-4 py-3 dark:bg-canvas-dark/40';
  const labelClass = harness.detected
    ? 'text-[13px] font-medium text-fg-primary dark:text-fg-primary-dark'
    : 'text-[13px] font-medium text-fg-secondary dark:text-fg-secondary-dark';

  const dot: DotStatus = harness.detected ? 'green' : 'gray';
  const pillTone: PillTone =
    harness.telemetry === 'on' ? 'green' : harness.telemetry === 'off' ? 'neutral' : 'amber';

  return (
    <li
      className={rowClass}
      data-testid={`harness-row-${harness.id}`}
      data-detected={harness.detected ? 'true' : 'false'}
    >
      <div className="flex min-w-0 items-center gap-3">
        <HarnessLogo id={harness.id} dimmed={!harness.detected} />
        <div className="min-w-0">
          <p className={labelClass}>{HARNESS_LABELS[harness.id]}</p>
          <p className="truncate text-[11px] text-fg-tertiary dark:text-fg-tertiary-dark">
            {detectionLabel}
          </p>
        </div>
      </div>
      <div className="flex items-center gap-2">
        <StatusDot status={dot} size="sm" pulse={false} />
        <Pill tone={pillTone} testid={`harness-telemetry-${harness.id}`}>
          {telemetryLabel}
        </Pill>
        {badge ? (
          <Popover
            icon={badge.icon}
            description={badge.description}
            {...(badge.docsUrl ? { docsUrl: badge.docsUrl } : {})}
            testid={`harness-badge-${harness.id}`}
          >
            <Pill tone={badge.tone} size="xs">
              {badge.label}
            </Pill>
          </Popover>
        ) : null}
        {/* Adapterless harnesses (e.g. Claude Desktop, which Trove
            auto-detects via its local audit log) have no user-facing
            action — the telemetry pill is the entire UI. Render the
            CTA only when an adapter exists or a setup guide is
            registered. */}
        {adapterAvailable || hasGuide ? (
          <Button
            variant={!adapterAvailable && hasGuide ? 'primary' : 'secondary'}
            size="sm"
            onClick={handleClick}
            disabled={!harness.detected || busy}
            aria-label={
              !adapterAvailable && hasGuide ? `setup-${harness.id}` : `toggle-${harness.id}`
            }
          >
            {buttonLabel}
          </Button>
        ) : null}
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
        ? `Detected via config; ${harness.configPath}`
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

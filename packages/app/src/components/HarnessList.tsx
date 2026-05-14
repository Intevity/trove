import { Search, X } from 'lucide-react';
import { useMemo, useState } from 'react';

import type { DetectedHarness, HarnessId } from '@trove/shared';

import { HARNESS_SETUP_GUIDES, HarnessSetupGuideModal } from './HarnessSetupGuide.js';
import {
  Button,
  Card,
  CardHeader,
  CardTitle,
  Pill,
  StatusDot,
  type DotStatus,
  type PillTone,
} from './ui/index.js';

const HARNESS_LABELS: Record<HarnessId, string> = {
  'claude-code': 'Claude Code',
  'claude-desktop': 'Claude Desktop',
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

/** Per-harness mark rendered as an inline SVG fallback to the left of
 *  each row. `bg` is the brand-aligned color of the rounded-square
 *  tile; `mark` is the 1–2 character monogram drawn in white at the
 *  centre. The SVG root carries no background, so the tile sits cleanly
 *  over either light or dark row backgrounds.
 *
 *  This fallback renders only when no real brand artwork is present in
 *  `packages/app/src/assets/harness-logos/<id>.svg`. To ship the real
 *  brand mark for a harness, drop a transparent-background SVG file at
 *  that path; Vite picks it up at build time via the `BRAND_LOGO_URLS`
 *  glob below and the component switches to `<img>` rendering. */
interface HarnessLogoSpec {
  bg: string;
  mark: string;
}
const HARNESS_LOGOS: Record<HarnessId, HarnessLogoSpec> = {
  'claude-code': { bg: '#CC785C', mark: 'C' },
  'claude-desktop': { bg: '#CC785C', mark: 'CD' },
  'gemini-cli': { bg: '#1A73E8', mark: 'G' },
  'codex-cli': { bg: '#10A37F', mark: 'O' },
  'qwen-code': { bg: '#FF6A00', mark: 'Q' },
  opencode: { bg: '#0F766E', mark: '{}' },
  'cursor-ide': { bg: '#0EA5E9', mark: 'C' },
  'cursor-cli': { bg: '#0284C7', mark: 'C$' },
  cline: { bg: '#EF4444', mark: 'CL' },
  aider: { bg: '#A855F7', mark: 'A' },
  'copilot-cli': { bg: '#24292E', mark: 'gh' },
};

const BRAND_LOGO_SOURCES = import.meta.glob<string>('../assets/harness-logos/*.svg', {
  eager: true,
  query: '?raw',
  import: 'default',
});

interface ParsedBrandLogo {
  viewBox: string;
  inner: string;
}

const PARSED_BRAND_LOGOS: Map<string, ParsedBrandLogo | null> = new Map();
for (const [path, raw] of Object.entries(BRAND_LOGO_SOURCES)) {
  PARSED_BRAND_LOGOS.set(path, parseBrandLogo(raw));
}

function parseBrandLogo(raw: string): ParsedBrandLogo | null {
  const open = raw.match(/<svg\b([^>]*)>/i);
  const close = raw.lastIndexOf('</svg>');
  if (!open || open.index === undefined || close === -1) return null;
  const openAttrs = open[1] ?? '';
  const viewBoxMatch = openAttrs.match(/viewBox\s*=\s*"([^"]+)"/i);
  const viewBox = viewBoxMatch?.[1];
  if (!viewBox) return null;
  const inner = raw.slice(open.index + open[0].length, close).trim();
  return { viewBox, inner };
}

function brandLogo(id: HarnessId): ParsedBrandLogo | undefined {
  return PARSED_BRAND_LOGOS.get(`../assets/harness-logos/${id}.svg`) ?? undefined;
}

interface CoverageNote {
  text: string;
  tooltip: string;
  docsUrl: string;
}
/** Small italic line shown under the telemetry Pill, for harnesses
 *  whose detection signal isn't a host config file Trove patches.
 *  Claude Desktop is the one entry today: there's no OTLP plumbing or
 *  hook to install — Trove reads each Cowork session's local audit log
 *  as it's appended and synthesises Tier A metrics in-stream. */
const TELEMETRY_HINTS: Partial<Record<HarnessId, string>> = {
  'claude-desktop':
    'Detected in-stream from each Cowork session’s local audit log — no setup needed.',
};

const COVERAGE_NOTES: Partial<Record<HarnessId, CoverageNote>> = {
  'cursor-cli': {
    text: 'Partial event coverage',
    tooltip:
      "Cursor CLI (cursor-agent) fires only a subset of Cursor's hook events — primarily beforeShellExecution and afterShellExecution. Cursor IDE fires the full surface. See Cursor's hooks docs.",
    docsUrl: 'https://cursor.com/docs/hooks',
  },
  cline: {
    text: 'Best-effort coverage',
    tooltip:
      "Cline doesn't emit OpenTelemetry natively. Trove watches Cline's per-task globalStorage records and emits OTLP logs derived from them. Token counts and durations are captured; raw conversation content stays on disk unless prompt logging is explicitly enabled.",
    docsUrl: 'https://github.com/cline/cline',
  },
  aider: {
    text: 'Best-effort coverage',
    tooltip:
      "Aider doesn't emit OpenTelemetry natively. Trove installs a shell-rc wrapper that runs the real aider and tees its session log; a watcher parses the log into OTLP records. Open a fresh terminal after enabling so the new shell function takes effect.",
    docsUrl: 'https://aider.chat/docs/',
  },
  'copilot-cli': {
    text: 'Best-effort coverage',
    tooltip:
      "GitHub Copilot CLI doesn't emit OpenTelemetry natively. Trove installs a shell-rc wrapper exposed as `gh-copilot` that runs `gh copilot` and logs invocation counts + durations. While Trove is enabled, invoke as `gh-copilot` (with a hyphen) instead of `gh copilot` so the wrapper observes the call.",
    docsUrl: 'https://docs.github.com/en/copilot/github-copilot-in-the-cli',
  },
};

export interface HarnessListProps {
  harnesses: DetectedHarness[];
  loading: boolean;
  onEnable?: (id: HarnessId) => void;
  onDisable?: (id: HarnessId) => void;
  busyIds?: ReadonlySet<HarnessId>;
  onRefresh?: () => void | Promise<void>;
}

export function HarnessList({
  harnesses,
  loading,
  onEnable,
  onDisable,
  busyIds,
  onRefresh,
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
            title="Re-run detection — useful when you've just installed a harness"
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
            className="w-full rounded-[8px] border border-hairline bg-surface-elevated py-1.5 pl-8 pr-8 text-[12px] text-fg-primary placeholder:text-fg-tertiary focus:border-ios-blue focus:outline-none focus:ring-1 focus:ring-ios-blue dark:border-hairline-dark dark:bg-surface-elevated-dark dark:text-fg-primary-dark dark:placeholder:text-fg-tertiary-dark"
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

function HarnessLogo({ id, dimmed }: { id: HarnessId; dimmed: boolean }): JSX.Element {
  const className = `shrink-0 ${dimmed ? 'opacity-40 grayscale' : ''}`;
  const brand = brandLogo(id);
  if (brand) {
    return (
      <svg
        width="32"
        height="32"
        viewBox={brand.viewBox}
        xmlns="http://www.w3.org/2000/svg"
        role="img"
        aria-label={`${HARNESS_LABELS[id]} logo`}
        data-testid={`harness-logo-${id}`}
        className={className}
        dangerouslySetInnerHTML={{ __html: brand.inner }}
      />
    );
  }

  const { bg, mark } = HARNESS_LOGOS[id];
  const fontSize = mark.length >= 2 ? 11 : 14;
  return (
    <svg
      width="32"
      height="32"
      viewBox="0 0 32 32"
      xmlns="http://www.w3.org/2000/svg"
      role="img"
      aria-label={`${HARNESS_LABELS[id]} logo`}
      data-testid={`harness-logo-${id}`}
      className={className}
    >
      <rect x="0" y="0" width="32" height="32" rx="8" fill={bg} />
      <text
        x="16"
        y="17"
        textAnchor="middle"
        dominantBaseline="central"
        fontFamily="system-ui, -apple-system, sans-serif"
        fontSize={fontSize}
        fontWeight="700"
        fill="#ffffff"
      >
        {mark}
      </text>
    </svg>
  );
}

interface HarnessRowProps {
  harness: DetectedHarness;
  onEnable: ((id: HarnessId) => void) | undefined;
  onDisable: ((id: HarnessId) => void) | undefined;
  busy: boolean;
  onOpenGuide: () => void;
}

function HarnessRow({
  harness,
  onEnable,
  onDisable,
  busy,
  onOpenGuide,
}: HarnessRowProps): JSX.Element {
  const adapterAvailable = harness.adapterAvailable;
  const guide = HARNESS_SETUP_GUIDES[harness.id];
  const hasGuide = Boolean(guide);
  const detectionLabel = describeDetection(harness);
  const telemetryLabel = describeTelemetry(harness);
  const enabled = harness.troveRegionPresent;

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
      <div className="flex items-center gap-3">
        <HarnessLogo id={harness.id} dimmed={!harness.detected} />
        <div>
          <p className={labelClass}>{HARNESS_LABELS[harness.id]}</p>
          <p className="text-[11px] text-fg-tertiary dark:text-fg-tertiary-dark">
            {detectionLabel}
          </p>
        </div>
      </div>
      <div className="flex flex-col items-end gap-1.5 text-right">
        <span className="flex items-center gap-1.5">
          <StatusDot status={dot} size="sm" pulse={false} />
          <Pill tone={pillTone} testid={`harness-telemetry-${harness.id}`}>
            {telemetryLabel}
          </Pill>
        </span>
        {TELEMETRY_HINTS[harness.id] ? (
          <p
            className="max-w-[260px] text-[10.5px] italic text-fg-tertiary dark:text-fg-tertiary-dark"
            data-testid={`harness-telemetry-hint-${harness.id}`}
            title={TELEMETRY_HINTS[harness.id]}
          >
            {TELEMETRY_HINTS[harness.id]}
          </p>
        ) : null}
        {COVERAGE_NOTES[harness.id] ? (
          <a
            className="text-[11px] italic text-ios-orange underline-offset-2 hover:underline"
            data-testid={`harness-coverage-note-${harness.id}`}
            href={COVERAGE_NOTES[harness.id]!.docsUrl}
            target="_blank"
            rel="noreferrer noopener"
            title={COVERAGE_NOTES[harness.id]!.tooltip}
          >
            {COVERAGE_NOTES[harness.id]!.text}
          </a>
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

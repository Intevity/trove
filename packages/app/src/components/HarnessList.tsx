import { Search, X } from 'lucide-react';
import { useMemo, useState } from 'react';

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

/** Build-time map from `<id>.svg` filename to its raw markup.
 *  Empty until a contributor drops a file in
 *  `packages/app/src/assets/harness-logos/`. `eager: true` so the
 *  lookup is synchronous in the render path; `query: '?raw'` returns
 *  the file contents as a string so we can inline the brand mark as
 *  an actual `<svg>` element rather than an `<img>` wrapper. Inlining
 *  matters because the row's `<svg>` itself must carry no `fill` (so
 *  the tile background composites transparently against either light
 *  or dark row surfaces) — we drop the source SVG's root attributes
 *  and only forward its `viewBox`, leaving brand colors on the inner
 *  paths to come through. */
const BRAND_LOGO_SOURCES = import.meta.glob<string>('../assets/harness-logos/*.svg', {
  eager: true,
  query: '?raw',
  import: 'default',
});

interface ParsedBrandLogo {
  viewBox: string;
  inner: string;
}

/** Cache so we parse each SVG once at module load instead of per render. */
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

/** Per-harness coverage advisory surfaced as a badge next to the
 *  telemetry status. Cursor CLI carries one because its hook surface
 *  is a strict subset of the IDE's; the Tier 3 trio (Cline, Aider,
 *  Copilot CLI) carry one because none of those tools emit native
 *  OTEL — Trove approximates their telemetry via log watching or
 *  shell-rc wrappers, with the trade-offs called out in the tooltip.
 *  The badge text stays short (the row is tight); the optional
 *  `tooltip` field renders as a `title` attribute so hovering surfaces
 *  the longer explanation without taking row real-estate. The
 *  `docsUrl` link points at the upstream tool's docs. */
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
  /** Called when the user clicks "Enable" — the parent opens the
   *  PatchPreviewModal for `id`. */
  onEnable?: (id: HarnessId) => void;
  /** Called when the user clicks "Disable" — the parent calls
   *  `revertPatch` for `id`. */
  onDisable?: (id: HarnessId) => void;
  /** Set of harness IDs whose row is currently mid-revert (button
   *  disables + label changes). */
  busyIds?: ReadonlySet<HarnessId>;
  /** Optional click handler for the header's Refresh button. Wired
   *  from the dashboard, which owns the `refresh()` returned by
   *  `useDetectedHarnesses`. When omitted (e.g. test fixtures that
   *  only care about row rendering), the button isn't rendered. */
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

  // Filter first (by name/id, case-insensitive), then partition detected
  // ahead of undetected so a user scanning the list lands on the rows
  // they can toggle without hunting. Stable partition preserves the
  // parent's relative order inside each group.
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
    <section data-testid="harness-list-section">
      <header className="mb-2 flex items-center justify-between">
        <h2 className="text-sm font-semibold text-slate-900 dark:text-slate-100">
          Detected harnesses
        </h2>
        {onRefresh ? (
          <button
            type="button"
            data-testid="harness-list-refresh"
            disabled={loading}
            onClick={() => void onRefresh()}
            className="inline-flex items-center gap-1 rounded border border-slate-300 bg-white px-2 py-0.5 text-xs font-medium text-slate-700 hover:bg-slate-100 disabled:cursor-not-allowed disabled:opacity-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-200 dark:hover:bg-slate-800"
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
          </button>
        ) : null}
      </header>

      {showSearch ? (
        <div className="relative mb-2">
          <Search
            size={13}
            aria-hidden="true"
            className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-slate-400"
          />
          <input
            type="text"
            role="searchbox"
            data-testid="harness-search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Filter harnesses by name…"
            aria-label="Filter harnesses by name"
            className="w-full rounded-md border border-slate-300 bg-white py-1.5 pl-8 pr-8 text-xs text-slate-800 shadow-sm focus:border-blue-500 focus:outline-none focus:ring-1 focus:ring-blue-500 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-200"
          />
          {hasQuery ? (
            <button
              type="button"
              data-testid="harness-search-clear"
              aria-label="Clear search"
              onClick={() => setQuery('')}
              className="absolute right-2 top-1/2 -translate-y-1/2 rounded p-0.5 text-slate-400 hover:text-slate-700 dark:hover:text-slate-200"
            >
              <X size={13} aria-hidden="true" />
            </button>
          ) : null}
        </div>
      ) : null}

      {loading ? (
        <p
          className="text-sm text-slate-500 dark:text-slate-400"
          data-testid="harness-list-loading"
        >
          Detecting harnesses…
        </p>
      ) : harnesses.length === 0 ? (
        <p className="text-sm text-slate-500 dark:text-slate-400" data-testid="harness-list-empty">
          No supported harnesses detected on this machine.
        </p>
      ) : sorted.length === 0 ? (
        <p
          className="text-sm text-slate-500 dark:text-slate-400"
          data-testid="harness-list-no-matches"
        >
          No harnesses match “{query.trim()}”.
        </p>
      ) : (
        <ul
          className="divide-y divide-slate-200 rounded-lg border border-slate-200 dark:divide-slate-800 dark:border-slate-800"
          data-testid="harness-list"
        >
          {sorted.map((harness) => (
            <HarnessRow
              key={harness.id}
              harness={harness}
              onEnable={onEnable}
              onDisable={onDisable}
              busy={busyIds?.has(harness.id) ?? false}
            />
          ))}
        </ul>
      )}
    </section>
  );
}

/** Brand mark for a harness. Renders the user-supplied SVG from
 *  `packages/app/src/assets/harness-logos/<id>.svg` when present;
 *  otherwise falls back to the inline monogram tile. Either path
 *  produces a 32x32 element with a transparent background that
 *  composites cleanly over light and dark row surfaces. `dimmed` greys
 *  out the logo for undetected rows to mirror the row's muted styling. */
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

  const rowClass = harness.detected
    ? 'flex items-center justify-between gap-4 border-l-2 border-l-emerald-400 bg-white px-4 py-3 dark:bg-slate-950'
    : 'flex items-center justify-between gap-4 border-l-2 border-l-transparent bg-slate-50/60 px-4 py-3 dark:bg-slate-900/40';
  const labelClass = harness.detected
    ? 'text-sm font-medium text-slate-900 dark:text-slate-100'
    : 'text-sm font-medium text-slate-500 dark:text-slate-400';

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
          <p className="text-xs text-slate-500 dark:text-slate-400">{detectionLabel}</p>
        </div>
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

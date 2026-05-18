import { Maximize2, Minimize2, Share2 } from 'lucide-react';
import { AnimatePresence, motion, useAnimationFrame } from 'motion/react';
import { useEffect, useRef, useState } from 'react';

import { presetMetadataFor } from '@trove/collector-presets';
import type {
  BackendInstance,
  CollectorRunState,
  HarnessConfig,
  MetricsSnapshotWire,
} from '@trove/shared';

import troveLogo from '../assets/trove-logo.svg';
import { HARNESS_LABELS, NodeLogoSvg } from '../lib/logos.js';
import { Card, CardHeader, CardTitle } from './ui/index.js';

interface FlowChartProps {
  harnesses: HarnessConfig[];
  backends: BackendInstance[];
  metrics: MetricsSnapshotWire | null;
  state: CollectorRunState | null;
}

type Mode = 'standard' | 'expanded';

/** Persist the user's view-mode choice across launches. WebKit
 *  localStorage survives Tauri restarts for the same app identifier. */
const MODE_STORAGE_KEY = 'trove.flow-chart.mode';

function readStoredMode(): Mode {
  try {
    const v = localStorage.getItem(MODE_STORAGE_KEY);
    if (v === 'expanded' || v === 'standard') return v;
  } catch {
    /* localStorage may throw in private mode or under sandbox; ignore. */
  }
  return 'standard';
}

function persistMode(mode: Mode): void {
  try {
    localStorage.setItem(MODE_STORAGE_KEY, mode);
  } catch {
    /* ignore */
  }
}

const LANE_COLORS = {
  spans: '#0A84FF',
  metrics: '#30D158',
  logs: '#FF9F0A',
} as const;

const LANES = [
  { kind: 'spans', label: 'Spans', accessor: 'spans' as const },
  { kind: 'metrics', label: 'Metrics', accessor: 'metricPoints' as const },
  { kind: 'logs', label: 'Logs', accessor: 'logRecords' as const },
] as const;

const VIEW_W = 800;
const HARNESS_COL_X = 110;
const COLLECTOR_X = 400;
const PLATFORM_COL_X = 700;
const COLLECTOR_HW = 70;
const COLLECTOR_HH = 36;
/** Floor for column half-width. Most labels fit; the actual hw grows
 *  to envelop the widest label in the column (see [`columnHalfWidth`]). */
const NODE_HW_MIN = 60;
/** Ceiling for column half-width — keeps columns from colliding with
 *  the collector when a user names a backend with a very long string. */
const NODE_HW_MAX = 95;
const NODE_HH_MAX = 24;
const COL_PADDING = 22;
const LANE_Y_OFFSETS = { spans: -13, metrics: 0, logs: 13 } as const;
const PARTICLES_PER_LANE = 5;

/** Inner-node geometry — kept aligned with FlowNode below. Logo +
 *  text padding + inter-glyph estimate combine to give a quick width
 *  estimate without mounting a measurement DOM. */
const NODE_PADDING_X = 8;
const NODE_LOGO_GAP = 6;
const NODE_LOGO_SIZE = 24;
const TITLE_AVG_GLYPH_PX = 6.8;

/** Estimate the rendered width of a node title at the chart's title
 *  font size. Adds logo + paddings so callers get a complete node
 *  width, not just text width. */
function estimateNodeWidth(title: string, hasLogo: boolean): number {
  const text = Math.ceil(title.length * TITLE_AVG_GLYPH_PX);
  const logoSlot = hasLogo ? NODE_LOGO_SIZE + NODE_LOGO_GAP : 0;
  return logoSlot + text + NODE_PADDING_X * 2;
}

/** Largest half-width any node in a column needs to fit its label.
 *  Clamped to [NODE_HW_MIN, NODE_HW_MAX] so every box in a column
 *  shares the same width regardless of which row holds the long
 *  string. */
function columnHalfWidth(titles: string[], hasLogo: boolean): number {
  const widest = titles.reduce((acc, t) => Math.max(acc, estimateNodeWidth(t, hasLogo)), 0);
  const hw = Math.ceil(widest / 2);
  return Math.min(NODE_HW_MAX, Math.max(NODE_HW_MIN, hw));
}

/** Compute total SVG height based on row count. Slimmer than the
 *  previous fixed 240 — standard mode renders at 150; expanded grows
 *  proportional to the bigger column. The legend now lives outside the
 *  SVG so we don't reserve space for it here. */
function computeViewHeight(rows: number): number {
  return Math.min(270, Math.max(150, 60 + rows * 36));
}

/** Distribute `count` row centers vertically across the canvas. */
function computeColumnCenters(count: number, viewH: number, padding = COL_PADDING): number[] {
  if (count <= 1) return [viewH / 2];
  const usable = viewH - 2 * padding;
  const step = usable / (count - 1);
  return Array.from({ length: count }, (_, i) => padding + step * i);
}

function periodForRate(rate: number): number {
  if (rate >= 10) return 1200;
  if (rate >= 1) return 2400;
  if (rate > 0) return 4800;
  return 0;
}

interface LaneRates {
  spans: number;
  metrics: number;
  logs: number;
}

function emptyLaneRates(): LaneRates {
  return { spans: 0, metrics: 0, logs: 0 };
}

function useReceivedRates(metrics: MetricsSnapshotWire | null): LaneRates {
  const prev = useRef<{ snap: MetricsSnapshotWire; at: number } | null>(null);
  const [rates, setRates] = useState<LaneRates>(emptyLaneRates);

  useEffect(() => {
    if (!metrics) return;
    const now = performance.now();
    const last = prev.current;
    if (!last) {
      prev.current = { snap: metrics, at: now };
      return;
    }
    const dtSec = (now - last.at) / 1000;
    if (dtSec <= 0) {
      prev.current = { snap: metrics, at: now };
      return;
    }
    const next = {
      spans: deltaPerSec(metrics.received.spans, last.snap.received.spans, dtSec),
      metrics: deltaPerSec(metrics.received.metricPoints, last.snap.received.metricPoints, dtSec),
      logs: deltaPerSec(metrics.received.logRecords, last.snap.received.logRecords, dtSec),
    };
    prev.current = { snap: metrics, at: now };
    setRates(next);
  }, [metrics]);

  return rates;
}

/** Derive per-harness rates from `metrics.diagObservations` deltas. The
 *  collector emits one diag pipeline per native-OTel emitter, so any
 *  enabled harness with a `service.name` candidate (Claude Code,
 *  Gemini, Codex, Qwen, OpenCode, Claude Desktop) gets its own entry.
 *  Watcher-emitter harnesses don't appear here; the caller falls back
 *  to aggregate animation for those. */
function usePerHarnessRates(metrics: MetricsSnapshotWire | null): Record<string, LaneRates> {
  const prev = useRef<{ snap: MetricsSnapshotWire; at: number } | null>(null);
  const [rates, setRates] = useState<Record<string, LaneRates>>({});

  useEffect(() => {
    if (!metrics) return;
    const now = performance.now();
    const last = prev.current;
    if (!last) {
      prev.current = { snap: metrics, at: now };
      return;
    }
    const dtSec = (now - last.at) / 1000;
    if (dtSec <= 0) {
      prev.current = { snap: metrics, at: now };
      return;
    }
    const next: Record<string, LaneRates> = {};
    const prevObs = last.snap.diagObservations ?? {};
    for (const [suffix, counts] of Object.entries(metrics.diagObservations ?? {})) {
      const p = prevObs[suffix];
      next[suffix] = {
        spans: deltaPerSec(counts.spans, p?.spans ?? 0, dtSec),
        metrics: deltaPerSec(counts.metricPoints, p?.metricPoints ?? 0, dtSec),
        logs: deltaPerSec(counts.logRecords, p?.logRecords ?? 0, dtSec),
      };
    }
    prev.current = { snap: metrics, at: now };
    setRates(next);
  }, [metrics]);

  return rates;
}

function deltaPerSec(curr: number, prev: number, dtSec: number): number {
  const delta = curr - prev;
  if (delta < 0) return 0;
  return delta / dtSec;
}

export function FlowChart({ harnesses, backends, metrics, state }: FlowChartProps): JSX.Element {
  const [mode, setMode] = useState<Mode>(readStoredMode);
  useEffect(() => {
    persistMode(mode);
  }, [mode]);
  const rates = useReceivedRates(metrics);
  const perHarnessRates = usePerHarnessRates(metrics);

  const periods = {
    spans: periodForRate(rates.spans),
    metrics: periodForRate(rates.metrics),
    logs: periodForRate(rates.logs),
  };
  const allIdle = periods.spans === 0 && periods.metrics === 0 && periods.logs === 0;
  const running = state?.kind === 'running';

  /** Pick the period for a harness's lane. In standard mode (one node
   *  represents all harnesses) we use the aggregate. In expanded mode,
   *  we look up the per-harness rate from `diagObservations`; native
   *  emitters get an accurate per-signal period, watcher-emitters
   *  (absent from diag) fall back to the aggregate so their lane still
   *  reflects "something is flowing" rather than going silent. */
  function laneRateForHarness(harnessId: import('@trove/shared').HarnessId | null): LaneRates {
    if (!harnessId || mode === 'standard') return rates;
    const direct = perHarnessRates[harnessId];
    if (direct) return direct;
    // Watcher-emitters (no diag pipeline) — fall back to aggregate.
    return rates;
  }

  const harnessRows = mode === 'expanded' ? Math.max(harnesses.length, 1) : 1;
  const backendRows = Math.max(backends.length, 1);
  const rows = Math.max(harnessRows, backendRows);
  const viewH = computeViewHeight(rows);
  const collectorCy = viewH / 2;

  const harnessCenters = computeColumnCenters(harnessRows, viewH);
  const backendCenters = computeColumnCenters(backendRows, viewH);
  const nodeHH = Math.min(NODE_HH_MAX, Math.floor((viewH - 50) / Math.max(rows, 1) / 2));

  // Per-column uniform width. Computed from the longest title in the
  // column so every node in the column visually matches, regardless of
  // which row holds the long string. Standard mode uses the collapsed
  // "Harnesses" title (always short) plus the backend titles.
  const harnessTitles =
    mode === 'expanded' && harnesses.length > 0
      ? harnesses.map((h) => HARNESS_LABELS[h.id] ?? h.id)
      : ['Harnesses']; // standard / empty case
  const backendTitles =
    backends.length > 0
      ? backends.map((b) => b.label ?? presetMetadataFor(b.backend.kind).label)
      : ['No platforms'];
  const harnessHW = columnHalfWidth(harnessTitles, true);
  const platformHW = columnHalfWidth(backendTitles, true);

  // SVG render height scales with viewBox so aspect stays sensible.
  const renderH = Math.round((viewH / 240) * 200);

  return (
    <Card testid="flow-chart" className="my-1.5">
      <CardHeader className="mb-3">
        <CardTitle className="flex items-center gap-1.5 whitespace-nowrap">
          <Share2 size={14} strokeWidth={2.4} className="text-brand" aria-hidden="true" />
          Data flow
        </CardTitle>
        <div className="flex items-center gap-2">
          <ModeToggle mode={mode} onChange={setMode} />
          <span className="flex items-center gap-1.5">
            <span className="relative flex h-2 w-2 items-center justify-center">
              <span
                className={`relative inline-block h-1.5 w-1.5 rounded-full ${
                  allIdle ? 'bg-fg-tertiary dark:bg-fg-tertiary-dark' : 'bg-ios-green'
                }`}
              />
              {!allIdle ? (
                <span className="absolute inline-flex h-2 w-2 animate-ping rounded-full bg-ios-green opacity-60" />
              ) : null}
            </span>
            <span className="text-[12px] text-fg-secondary dark:text-fg-secondary-dark">
              {allIdle ? 'idle' : 'live'}
            </span>
          </span>
        </div>
      </CardHeader>

      <svg
        viewBox={`0 0 ${VIEW_W} ${viewH}`}
        role="img"
        aria-label="Telemetry flow from harnesses through the collector to configured platforms"
        className="w-full"
        style={{ height: renderH }}
        data-testid="flow-chart-svg"
      >
        <defs>
          <filter id="lane-glow" x="-50%" y="-50%" width="200%" height="200%">
            <feGaussianBlur stdDeviation="1.4" result="blur" />
            <feMerge>
              <feMergeNode in="blur" />
              <feMergeNode in="SourceGraphic" />
            </feMerge>
          </filter>
        </defs>

        {/* Incoming lanes (Harnesses → Collector). One set per harness row.
            In standard mode the single row uses the aggregate rate. In
            expanded mode each row uses its harness's per-signal rate
            from `diagObservations` so an idle harness's lane stays still
            while the active one streams particles. */}
        {harnessCenters.map((hcy, hi) => {
          const harnessId = mode === 'expanded' ? (harnesses[hi]?.id ?? null) : null;
          const laneRates = laneRateForHarness(harnessId);
          return LANES.map((lane) => (
            <FlowLane
              key={`in-${hi}-${lane.kind}`}
              pathD={connectorPath(
                HARNESS_COL_X + harnessHW,
                hcy + LANE_Y_OFFSETS[lane.kind],
                COLLECTOR_X - COLLECTOR_HW,
                collectorCy + LANE_Y_OFFSETS[lane.kind],
              )}
              color={LANE_COLORS[lane.kind]}
              periodMs={periodForRate(
                lane.kind === 'spans'
                  ? laneRates.spans
                  : lane.kind === 'metrics'
                    ? laneRates.metrics
                    : laneRates.logs,
              )}
            />
          ));
        })}

        {/* Outgoing lanes (Collector → Platforms). One set per backend row. */}
        {backendCenters.map((bcy, bi) =>
          LANES.map((lane) => (
            <FlowLane
              key={`out-${bi}-${lane.kind}`}
              pathD={connectorPath(
                COLLECTOR_X + COLLECTOR_HW,
                collectorCy + LANE_Y_OFFSETS[lane.kind],
                PLATFORM_COL_X - platformHW,
                bcy + LANE_Y_OFFSETS[lane.kind],
              )}
              color={LANE_COLORS[lane.kind]}
              periodMs={periods[lane.kind]}
            />
          )),
        )}

        {/* Per-lane rate labels (expanded mode only, when non-idle). */}
        {mode === 'expanded' && !allIdle ? (
          <g>
            {LANES.map((lane) => {
              const r = rates[lane.kind as 'spans' | 'metrics' | 'logs'];
              if (r === 0) return null;
              const labelX = (COLLECTOR_X + COLLECTOR_HW + (PLATFORM_COL_X - platformHW)) / 2;
              const labelY = collectorCy + LANE_Y_OFFSETS[lane.kind] - 3;
              return (
                <text
                  key={`rate-${lane.kind}`}
                  x={labelX}
                  y={labelY}
                  textAnchor="middle"
                  fontSize={9}
                  fontWeight={600}
                  className="fill-fg-secondary dark:fill-fg-secondary-dark"
                >
                  {Math.round(r)}/s
                </text>
              );
            })}
          </g>
        ) : null}

        {/* Collector halo pulse when running */}
        {running ? (
          <rect
            x={COLLECTOR_X - COLLECTOR_HW - 2}
            y={collectorCy - COLLECTOR_HH - 2}
            width={(COLLECTOR_HW + 2) * 2}
            height={(COLLECTOR_HH + 2) * 2}
            rx={12}
            ry={12}
            fill="none"
            stroke="#0A84FF"
            strokeWidth={1.5}
            strokeOpacity={0.35}
          >
            <animate
              attributeName="stroke-opacity"
              values="0.35;0;0.35"
              dur="2.4s"
              repeatCount="indefinite"
            />
            <animate
              attributeName="stroke-width"
              values="1.5;5;1.5"
              dur="2.4s"
              repeatCount="indefinite"
            />
          </rect>
        ) : null}

        {/* Harness column */}
        <AnimatePresence mode="popLayout" initial={false}>
          {mode === 'standard' ? (
            <motion.g
              key="harness-collapsed"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.18 }}
            >
              <FlowNode
                cx={HARNESS_COL_X}
                cy={harnessCenters[0]!}
                hw={harnessHW}
                hh={NODE_HH_MAX}
                title="Harnesses"
                subtitle={`${harnesses.length} active`}
                muted={harnesses.length === 0}
              />
            </motion.g>
          ) : harnesses.length === 0 ? (
            <motion.g
              key="harness-empty"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.18 }}
            >
              <FlowNode
                cx={HARNESS_COL_X}
                cy={harnessCenters[0]!}
                hw={harnessHW}
                hh={NODE_HH_MAX}
                title="No harnesses"
                subtitle="enable one"
                muted
              />
            </motion.g>
          ) : (
            harnessCenters.map((hcy, i) => {
              const h = harnesses[i]!;
              return (
                <motion.g
                  key={`harness-${h.id}`}
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  exit={{ opacity: 0 }}
                  transition={{ duration: 0.2 }}
                >
                  <FlowNode
                    cx={HARNESS_COL_X}
                    cy={hcy}
                    hw={harnessHW}
                    hh={nodeHH}
                    title={HARNESS_LABELS[h.id] ?? h.id}
                    harnessId={h.id}
                  />
                </motion.g>
              );
            })
          )}
        </AnimatePresence>

        {/* Collector node */}
        <FlowNode
          cx={COLLECTOR_X}
          cy={collectorCy}
          hw={COLLECTOR_HW}
          hh={COLLECTOR_HH}
          title="Collector"
          subtitle={running ? 'running' : (state?.kind ?? 'idle')}
          highlight
        />

        {/* Platform column */}
        {backends.length === 0 ? (
          <FlowNode
            cx={PLATFORM_COL_X}
            cy={viewH / 2}
            hw={platformHW}
            hh={NODE_HH_MAX}
            title="No platforms"
            subtitle="not forwarding"
            muted
          />
        ) : (
          backendCenters.map((bcy, i) => {
            const instance = backends[i]!;
            const meta = presetMetadataFor(instance.backend.kind);
            return (
              <motion.g
                key={instance.id}
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                transition={{ duration: 0.18 }}
              >
                <FlowNode
                  cx={PLATFORM_COL_X}
                  cy={bcy}
                  hw={platformHW}
                  hh={nodeHH}
                  title={instance.label ?? meta.label}
                  backendKind={instance.backend.kind}
                />
              </motion.g>
            );
          })
        )}
      </svg>

      <div className="mt-3 flex items-center gap-3 px-1">
        {LANES.map((lane) => (
          <span key={lane.kind} className="flex items-center gap-1.5">
            <span
              className="inline-block h-2 w-2 rounded-full"
              style={{ backgroundColor: LANE_COLORS[lane.kind] }}
            />
            <span className="text-[10px] text-fg-secondary dark:text-fg-secondary-dark">
              {lane.label}
            </span>
          </span>
        ))}
        {allIdle ? (
          <span className="ml-auto text-[10px] italic text-fg-tertiary dark:text-fg-tertiary-dark">
            Waiting for telemetry…
          </span>
        ) : null}
      </div>
    </Card>
  );
}

interface ModeToggleProps {
  mode: Mode;
  onChange: (m: Mode) => void;
}

/** macOS-style segmented icon pair. Two square buttons in a hairline
 *  pill; the active one renders with an elevated surface so it reads
 *  as recessed. */
function ModeToggle({ mode, onChange }: ModeToggleProps): JSX.Element {
  return (
    <div
      role="group"
      aria-label="Data flow view"
      className="inline-flex items-center gap-0.5 rounded-[7px] border border-hairline p-0.5 dark:border-hairline-dark"
    >
      <ToggleButton
        active={mode === 'standard'}
        onClick={() => onChange('standard')}
        label="Standard view"
        testid="flow-chart-mode-standard"
      >
        <Minimize2 size={11} aria-hidden />
      </ToggleButton>
      <ToggleButton
        active={mode === 'expanded'}
        onClick={() => onChange('expanded')}
        label="Expanded view"
        testid="flow-chart-mode-expanded"
      >
        <Maximize2 size={11} aria-hidden />
      </ToggleButton>
    </div>
  );
}

function ToggleButton({
  active,
  onClick,
  label,
  testid,
  children,
}: {
  active: boolean;
  onClick: () => void;
  label: string;
  testid: string;
  children: React.ReactNode;
}): JSX.Element {
  return (
    <button
      type="button"
      aria-label={label}
      aria-pressed={active}
      onClick={onClick}
      data-testid={testid}
      className={`flex items-center justify-center rounded-[5px] px-1.5 py-1 transition-colors ${
        active
          ? 'bg-surface-elevated text-fg-primary shadow-sm dark:bg-surface-elevated-dark dark:text-fg-primary-dark'
          : 'text-fg-tertiary hover:text-fg-secondary dark:text-fg-tertiary-dark dark:hover:text-fg-secondary-dark'
      }`}
    >
      {children}
    </button>
  );
}

interface FlowLaneProps {
  pathD: string;
  color: string;
  periodMs: number;
  /** Suppress particle + dash-flow animation while keeping the base
   *  line rendered. Used for incoming lanes in expanded mode where
   *  per-harness attribution is not available — drawing animated
   *  particles per harness would falsely imply each is emitting. */
  staticLane?: boolean;
}

/** A single lane: dim base path, an animated dashed "river" beneath
 *  the particles, and a glowing particle train. Skipped when the
 *  lane is idle (periodMs === 0) or marked static. */
function FlowLane({ pathD, color, periodMs, staticLane = false }: FlowLaneProps): JSX.Element {
  const pathRef = useRef<SVGPathElement | null>(null);
  const particleRefs = useRef<(SVGCircleElement | null)[]>([]);
  const startRef = useRef<number | null>(null);
  const idle = periodMs === 0;
  const animated = !idle && !staticLane;

  useAnimationFrame((time) => {
    if (!animated) return;
    const path = pathRef.current;
    if (!path) return;
    const total = path.getTotalLength();
    if (total === 0) return;
    if (startRef.current === null) startRef.current = time;
    const elapsed = time - startRef.current;
    for (let i = 0; i < PARTICLES_PER_LANE; i++) {
      const circle = particleRefs.current[i];
      if (!circle) continue;
      const phase = ((elapsed + (i * periodMs) / PARTICLES_PER_LANE) % periodMs) / periodMs;
      const pt = path.getPointAtLength(phase * total);
      circle.setAttribute('cx', String(pt.x));
      circle.setAttribute('cy', String(pt.y));
    }
  });

  // Dash flow dur: scale with period so faster lanes stream faster.
  const dashDur = animated ? `${Math.max(0.6, periodMs / 1000 / 2)}s` : '0s';
  // Static lanes get the normal-weight base stroke so the user still
  // sees "this harness is wired up", just without per-harness particles.
  const baseOpacity = idle ? 0.12 : 0.28;

  return (
    <g>
      <path
        ref={pathRef}
        d={pathD}
        stroke={color}
        strokeOpacity={baseOpacity}
        strokeWidth={1.5}
        fill="none"
      />
      {animated ? (
        <path
          d={pathD}
          stroke={color}
          strokeOpacity={0.45}
          strokeWidth={1.5}
          fill="none"
          strokeDasharray="3 9"
          strokeLinecap="round"
        >
          <animate
            attributeName="stroke-dashoffset"
            from="12"
            to="0"
            dur={dashDur}
            repeatCount="indefinite"
          />
        </path>
      ) : null}
      {animated &&
        Array.from({ length: PARTICLES_PER_LANE }, (_, i) => (
          <circle
            key={i}
            ref={(el) => {
              particleRefs.current[i] = el;
            }}
            r={2.4}
            fill={color}
            opacity={0.95}
            filter="url(#lane-glow)"
          />
        ))}
    </g>
  );
}

interface FlowNodeProps {
  cx: number;
  cy: number;
  hw: number;
  hh: number;
  title: string;
  subtitle?: string;
  highlight?: boolean;
  muted?: boolean;
  /** When set, renders the harness brand logo (or monogram fallback)
   *  inside the node and shifts the text right of it. */
  harnessId?: import('@trove/shared').HarnessId;
  /** When set, renders the backend monogram badge. */
  backendKind?: import('@trove/shared').Backend['kind'];
}

function FlowNode({
  cx,
  cy,
  hw,
  hh,
  title,
  subtitle,
  highlight,
  muted,
  harnessId,
  backendKind,
}: FlowNodeProps): JSX.Element {
  const x = cx - hw;
  const y = cy - hh;
  const hasLogo = harnessId !== undefined || backendKind !== undefined;
  const logoSize = hasLogo ? Math.min(hh * 1.4, 24) : 0;
  const logoX = x + 6;
  const logoY = cy - logoSize / 2;
  // With a logo: left-align the text starting just right of the logo.
  // Without: keep the original centered layout (used by the Collector node).
  const textX = hasLogo ? logoX + logoSize + 6 : cx;
  const textAnchor: 'start' | 'middle' = hasLogo ? 'start' : 'middle';
  // Title baseline: vertically center when there's no subtitle line under
  // it; otherwise keep the original two-line layout (title above, subtitle
  // below). The highlight branch leaves a touch more room between the
  // lines since the type is set larger.
  const titleY = subtitle ? (highlight ? cy - 3 : cy - 2) : cy + 4;
  const subtitleY = highlight ? cy + 14 : cy + 12;
  // Collector watermark: trove logo at low opacity behind the title.
  // Sized to fill the node's vertical extent without crowding the text;
  // pinned to the right half so it sits behind / beside the centered
  // title rather than under it.
  const watermarkSize = hh * 1.6;
  return (
    <g>
      <rect
        x={x}
        y={y}
        width={hw * 2}
        height={hh * 2}
        rx={10}
        ry={10}
        className={
          muted
            ? 'fill-surface-elevated stroke-hairline dark:fill-surface-elevated-dark dark:stroke-hairline-dark'
            : highlight
              ? 'fill-brand/[0.06] stroke-brand/55 dark:fill-brand/[0.10]'
              : 'fill-surface-elevated stroke-hairline dark:fill-surface-elevated-dark dark:stroke-hairline-dark'
        }
        strokeWidth={highlight ? 1.5 : 1}
      />
      {highlight && !hasLogo ? (
        <image
          href={troveLogo}
          x={cx - watermarkSize / 2}
          y={cy - watermarkSize / 2}
          width={watermarkSize}
          height={watermarkSize}
          opacity={0.1}
          preserveAspectRatio="xMidYMid meet"
          aria-hidden="true"
        />
      ) : null}
      {hasLogo ? (
        <NodeLogoSvg
          x={logoX}
          y={logoY}
          size={logoSize}
          {...(harnessId ? { harnessId } : {})}
          {...(backendKind ? { backendKind } : {})}
        />
      ) : null}
      <text
        x={textX}
        y={titleY}
        textAnchor={textAnchor}
        fontSize={highlight ? 14 : 12}
        fontWeight={600}
        className={
          muted
            ? 'fill-fg-tertiary dark:fill-fg-tertiary-dark'
            : 'fill-fg-primary dark:fill-fg-primary-dark'
        }
        style={highlight ? { filter: 'drop-shadow(0 1px 1.5px rgba(0,0,0,0.25))' } : undefined}
      >
        {title}
      </text>
      {subtitle ? (
        <text
          x={textX}
          y={subtitleY}
          textAnchor={textAnchor}
          fontSize={highlight ? 12 : 10}
          className={
            muted
              ? 'fill-fg-tertiary dark:fill-fg-tertiary-dark'
              : 'fill-fg-secondary dark:fill-fg-secondary-dark'
          }
          style={highlight ? { filter: 'drop-shadow(0 1px 1.5px rgba(0,0,0,0.2))' } : undefined}
        >
          {subtitle}
        </text>
      ) : null}
    </g>
  );
}

/** Cubic-bezier connector between two points on different columns.
 *  Control points sit at the horizontal midpoint so the curve has a
 *  natural S-shape regardless of vertical separation. */
function connectorPath(startX: number, startY: number, endX: number, endY: number): string {
  const c1x = startX + (endX - startX) * 0.5;
  const c2x = startX + (endX - startX) * 0.5;
  return `M ${startX} ${startY} C ${c1x} ${startY}, ${c2x} ${endY}, ${endX} ${endY}`;
}

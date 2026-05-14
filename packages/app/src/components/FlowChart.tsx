import { useEffect, useRef, useState } from 'react';

import { useAnimationFrame } from 'motion/react';
import { presetMetadataFor } from '@trove/collector-presets';
import type { BackendInstance, CollectorRunState, MetricsSnapshotWire } from '@trove/shared';

import { Card, CardHeader, CardTitle, StatusDot } from './ui/index.js';

interface FlowChartProps {
  harnessCount: number;
  backends: BackendInstance[];
  metrics: MetricsSnapshotWire | null;
  state: CollectorRunState | null;
}

/** Per-signal-type lane color. Matches the iOS palette tokens used
 *  elsewhere in the dashboard so the chart reads as part of the same
 *  visual language. */
const LANE_COLORS = {
  spans: '#0A84FF', // ios-blue
  metrics: '#30D158', // ios-green
  logs: '#FF9F0A', // ios-orange
} as const;

/** SignalKind → label / accessor. */
const LANES = [
  { kind: 'spans', label: 'Spans', accessor: 'spans' as const },
  { kind: 'metrics', label: 'Metrics', accessor: 'metricPoints' as const },
  { kind: 'logs', label: 'Logs', accessor: 'logRecords' as const },
] as const;

/** Fixed-coordinate viewBox the SVG uses; the wrapping element scales
 *  the whole thing to its container width while preserving aspect.
 *  240 vertical units gives enough room for up to 4 platform sub-nodes
 *  without crowding; more than that and the rows shrink proportionally. */
const VIEW_W = 800;
const VIEW_H = 240;

/** Node geometry. Centers + half-extents in viewBox units. */
const HARNESS_NODE = { cx: 110, cy: VIEW_H / 2, hw: 60, hh: 30 };
const COLLECTOR_NODE = { cx: 400, cy: VIEW_H / 2, hw: 70, hh: 38 };
const PLATFORM_COL_X = 700;
const PLATFORM_NODE_HW = 60;
const PLATFORM_NODE_HH_MAX = 26;

/** y-offsets per signal lane around a node's center. */
const LANE_Y_OFFSETS = { spans: -14, metrics: 0, logs: 14 } as const;

/** Number of particles that orbit each lane, regardless of throughput.
 *  Particle period (not count) is what scales with rate. */
const PARTICLES_PER_LANE = 5;

/** Throughput → particle period (ms). Three bands so the chart reads
 *  differently at idle, casual, and peak rates without being noisy
 *  about precise values. */
function periodForRate(rate: number): number {
  if (rate >= 10) return 1200;
  if (rate >= 1) return 2400;
  if (rate > 0) return 4800;
  return 0; // sentinel: lane is idle
}

/** Track received-counter deltas across snapshots to derive a rate per
 *  second per signal type. The first snapshot establishes a baseline;
 *  subsequent snapshots produce a rate from the delta. Falls back to 0
 *  when the counter went backward (collector restart) or when there's
 *  no snapshot yet. */
function useReceivedRates(metrics: MetricsSnapshotWire | null): {
  spans: number;
  metrics: number;
  logs: number;
} {
  const prev = useRef<{ snap: MetricsSnapshotWire; at: number } | null>(null);
  const [rates, setRates] = useState({ spans: 0, metrics: 0, logs: 0 });

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

function deltaPerSec(curr: number, prev: number, dtSec: number): number {
  const delta = curr - prev;
  if (delta < 0) return 0; // collector restart resets counters
  return delta / dtSec;
}

export function FlowChart({ harnessCount, backends, metrics, state }: FlowChartProps): JSX.Element {
  const rates = useReceivedRates(metrics);

  // Per-signal periods. When all three are zero the chart goes idle
  // (particles hidden, lanes dimmed) so the user sees at a glance that
  // nothing is flowing yet.
  const periods = {
    spans: periodForRate(rates.spans),
    metrics: periodForRate(rates.metrics),
    logs: periodForRate(rates.logs),
  };
  const allIdle = periods.spans === 0 && periods.metrics === 0 && periods.logs === 0;

  // Platform sub-node geometry. Distribute centers vertically so the
  // outermost rows still fit inside VIEW_H minus a small margin.
  const platformCenters = computePlatformCenters(
    Math.max(backends.length, 1), // 1 placeholder sub-node when zero configured
  );
  const platformNodeHH = Math.min(
    PLATFORM_NODE_HH_MAX,
    Math.floor((VIEW_H - 40) / (backends.length || 1) / 2),
  );

  return (
    <Card testid="flow-chart">
      <CardHeader>
        <CardTitle>Data flow</CardTitle>
        <span className="flex items-center gap-1.5">
          <StatusDot status={state?.kind === 'running' ? 'green' : 'gray'} size="sm" />
          <span className="text-[12px] text-fg-secondary dark:text-fg-secondary-dark">
            {allIdle ? 'idle' : 'live'}
          </span>
        </span>
      </CardHeader>

      <svg
        viewBox={`0 0 ${VIEW_W} ${VIEW_H}`}
        role="img"
        aria-label="Telemetry flow from harnesses through the collector to configured platforms"
        className="h-[200px] w-full"
        data-testid="flow-chart-svg"
      >
        {/* Connectors first so node fills paint on top */}
        {LANES.map((lane) => (
          <FlowLane
            key={`in-${lane.kind}`}
            pathD={incomingPathD(lane.kind)}
            color={LANE_COLORS[lane.kind]}
            periodMs={periods[lane.kind]}
          />
        ))}
        {platformCenters.map((cy, i) =>
          LANES.map((lane) => (
            <FlowLane
              key={`out-${i}-${lane.kind}`}
              pathD={outgoingPathD(lane.kind, cy)}
              color={LANE_COLORS[lane.kind]}
              periodMs={periods[lane.kind]}
            />
          )),
        )}

        {/* Harnesses node */}
        <FlowNode
          cx={HARNESS_NODE.cx}
          cy={HARNESS_NODE.cy}
          hw={HARNESS_NODE.hw}
          hh={HARNESS_NODE.hh}
          title="Harnesses"
          subtitle={`${harnessCount} active`}
        />

        {/* Collector node */}
        <FlowNode
          cx={COLLECTOR_NODE.cx}
          cy={COLLECTOR_NODE.cy}
          hw={COLLECTOR_NODE.hw}
          hh={COLLECTOR_NODE.hh}
          title="Collector"
          subtitle={state?.kind === 'running' ? 'running' : (state?.kind ?? 'idle')}
          highlight
        />

        {/* Platforms */}
        {backends.length === 0 ? (
          <FlowNode
            cx={PLATFORM_COL_X}
            cy={VIEW_H / 2}
            hw={PLATFORM_NODE_HW}
            hh={PLATFORM_NODE_HH_MAX}
            title="No platforms"
            subtitle="not forwarding"
            muted
          />
        ) : (
          backends.map((instance, i) => {
            const meta = presetMetadataFor(instance.backend.kind);
            return (
              <FlowNode
                key={instance.id}
                cx={PLATFORM_COL_X}
                cy={platformCenters[i]!}
                hw={PLATFORM_NODE_HW}
                hh={platformNodeHH}
                title={instance.label ?? meta.label}
                subtitle={meta.label}
              />
            );
          })
        )}

        {/* Lane legend at the bottom-left */}
        <g transform={`translate(20, ${VIEW_H - 14})`}>
          {LANES.map((lane, i) => (
            <g key={lane.kind} transform={`translate(${i * 70}, 0)`}>
              <circle cx={0} cy={0} r={3.5} fill={LANE_COLORS[lane.kind]} />
              <text
                x={8}
                y={3}
                fontSize={10}
                className="fill-fg-secondary dark:fill-fg-secondary-dark"
              >
                {lane.label}
              </text>
            </g>
          ))}
        </g>

        {allIdle ? (
          <text
            x={VIEW_W / 2}
            y={VIEW_H - 16}
            textAnchor="middle"
            fontSize={11}
            className="fill-fg-tertiary dark:fill-fg-tertiary-dark"
          >
            Waiting for telemetry…
          </text>
        ) : null}
      </svg>
    </Card>
  );
}

/** Imperatively animates `PARTICLES_PER_LANE` circles along a single
 *  bezier path. Skipped (and particles hidden) when `periodMs === 0`
 *  so an idle lane doesn't pin a rAF callback. */
function FlowLane({
  pathD,
  color,
  periodMs,
}: {
  pathD: string;
  color: string;
  periodMs: number;
}): JSX.Element {
  const pathRef = useRef<SVGPathElement | null>(null);
  const particleRefs = useRef<(SVGCircleElement | null)[]>([]);
  const startRef = useRef<number | null>(null);
  const idle = periodMs === 0;

  useAnimationFrame((time) => {
    if (idle) return;
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

  return (
    <g>
      <path
        ref={pathRef}
        d={pathD}
        stroke={color}
        strokeOpacity={idle ? 0.12 : 0.28}
        strokeWidth={1.5}
        fill="none"
      />
      {!idle &&
        Array.from({ length: PARTICLES_PER_LANE }, (_, i) => (
          <circle
            key={i}
            ref={(el) => {
              particleRefs.current[i] = el;
            }}
            r={2.6}
            fill={color}
            opacity={0.95}
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
}

/** macOS-native pill-shaped node. Single hairline border + subtle
 *  surface fill so the chart reads as part of the same visual language
 *  as the rest of the dashboard. */
function FlowNode({
  cx,
  cy,
  hw,
  hh,
  title,
  subtitle,
  highlight,
  muted,
}: FlowNodeProps): JSX.Element {
  const x = cx - hw;
  const y = cy - hh;
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
              ? 'fill-surface-elevated stroke-ios-blue/40 dark:fill-surface-elevated-dark'
              : 'fill-surface-elevated stroke-hairline dark:fill-surface-elevated-dark dark:stroke-hairline-dark'
        }
        strokeWidth={highlight ? 1.5 : 1}
      />
      <text
        x={cx}
        y={cy - 2}
        textAnchor="middle"
        fontSize={12}
        fontWeight={600}
        className={
          muted
            ? 'fill-fg-tertiary dark:fill-fg-tertiary-dark'
            : 'fill-fg-primary dark:fill-fg-primary-dark'
        }
      >
        {title}
      </text>
      {subtitle ? (
        <text
          x={cx}
          y={cy + 12}
          textAnchor="middle"
          fontSize={10}
          className={
            muted
              ? 'fill-fg-tertiary dark:fill-fg-tertiary-dark'
              : 'fill-fg-secondary dark:fill-fg-secondary-dark'
          }
        >
          {subtitle}
        </text>
      ) : null}
    </g>
  );
}

/** Cubic-bezier path from Harnesses right-edge to Collector left-edge
 *  for `lane`. Slight vertical offset per signal type so the three
 *  lanes don't overlap visually. */
function incomingPathD(lane: 'spans' | 'metrics' | 'logs'): string {
  const startX = HARNESS_NODE.cx + HARNESS_NODE.hw;
  const endX = COLLECTOR_NODE.cx - COLLECTOR_NODE.hw;
  const startY = HARNESS_NODE.cy + LANE_Y_OFFSETS[lane];
  const endY = COLLECTOR_NODE.cy + LANE_Y_OFFSETS[lane];
  const c1x = startX + (endX - startX) * 0.45;
  const c2x = startX + (endX - startX) * 0.55;
  return `M ${startX} ${startY} C ${c1x} ${startY}, ${c2x} ${endY}, ${endX} ${endY}`;
}

/** Cubic-bezier from Collector right-edge to a Platforms sub-node
 *  centered at `platformCy`. Lane Y-offset is applied at both ends so
 *  the three lanes remain parallel through the fan-out. */
function outgoingPathD(lane: 'spans' | 'metrics' | 'logs', platformCy: number): string {
  const startX = COLLECTOR_NODE.cx + COLLECTOR_NODE.hw;
  const endX = PLATFORM_COL_X - PLATFORM_NODE_HW;
  const startY = COLLECTOR_NODE.cy + LANE_Y_OFFSETS[lane];
  const endY = platformCy + LANE_Y_OFFSETS[lane];
  const c1x = startX + (endX - startX) * 0.5;
  const c2x = startX + (endX - startX) * 0.5;
  return `M ${startX} ${startY} C ${c1x} ${startY}, ${c2x} ${endY}, ${endX} ${endY}`;
}

/** Distribute `count` platform sub-node centers vertically so the
 *  outermost rows fit inside the canvas with reasonable padding. */
function computePlatformCenters(count: number): number[] {
  const padding = 30;
  const usable = VIEW_H - 2 * padding;
  if (count === 1) return [VIEW_H / 2];
  const step = usable / (count - 1);
  return Array.from({ length: count }, (_, i) => padding + step * i);
}

// Shared logo + label primitives for harnesses and forwarding-backend
// kinds. Both the HarnessList row, the Platforms tab row, and the
// FlowChart nodes pull their iconography from here so the visual
// language stays consistent across surfaces.

import type { Backend, HarnessId } from '@trove/shared';

export const HARNESS_LABELS: Record<HarnessId, string> = {
  'claude-code': 'Claude Code',
  'claude-desktop': 'Claude Desktop',
  'gemini-cli': 'Gemini CLI',
  'codex-cli': 'OpenAI Codex CLI',
  'codex-desktop': 'OpenAI Codex',
  'qwen-code': 'Qwen Code',
  opencode: 'OpenCode',
  'cursor-ide': 'Cursor IDE',
  'cursor-cli': 'Cursor CLI',
  cline: 'Cline',
  aider: 'Aider',
  'copilot-cli': 'GitHub Copilot CLI',
  'junie-cli': 'Junie CLI',
  droid: 'Droid',
  'kimi-code-cli': 'Kimi Code CLI',
  devin: 'Devin',
  forgecode: 'ForgeCode',
  sentinel: 'Sentinel',
};

/** Harnesses not yet fully validated in the harness × platform matrix
 *  (`documentation/harness-platform-matrix.md`) — they have no end-to-end
 *  PASS recorded. The Harnesses list surfaces these with a "Beta" pill. */
export const HARNESS_BETA: ReadonlySet<HarnessId> = new Set<HarnessId>([
  'junie-cli',
  'droid',
  'kimi-code-cli',
  'devin',
  'forgecode',
  'sentinel',
]);

/** Per-harness row overrides for the Harnesses list. `titleOverride`
 *  replaces the {@link HARNESS_LABELS} title (kept short elsewhere — e.g.
 *  FlowChart nodes — so only the row is affected); `learnMoreUrl` renders a
 *  "Learn More" link next to the title. */
export const HARNESS_ROW_META: Partial<
  Record<HarnessId, { titleOverride?: string; learnMoreUrl?: string }>
> = {
  sentinel: {
    titleOverride: 'Sentinel - A Claude Code companion',
    learnMoreUrl: 'https://github.com/Intevity/sentinel',
  },
};

export interface LogoFallback {
  /** Tile background. Brand-aligned where the vendor has a recognisable
   *  primary colour; otherwise a neutral that still reads on light + dark. */
  bg: string;
  /** 1–2 character monogram drawn in white at tile centre. */
  mark: string;
}

export const HARNESS_LOGOS: Record<HarnessId, LogoFallback> = {
  'claude-code': { bg: '#CC785C', mark: 'C' },
  'claude-desktop': { bg: '#CC785C', mark: 'CD' },
  'gemini-cli': { bg: '#1A73E8', mark: 'G' },
  'codex-cli': { bg: '#10A37F', mark: 'O' },
  'codex-desktop': { bg: '#10A37F', mark: 'O' },
  'qwen-code': { bg: '#FF6A00', mark: 'Q' },
  opencode: { bg: '#0F766E', mark: '{}' },
  'cursor-ide': { bg: '#0EA5E9', mark: 'C' },
  'cursor-cli': { bg: '#0284C7', mark: 'C$' },
  cline: { bg: '#EF4444', mark: 'CL' },
  aider: { bg: '#A855F7', mark: 'A' },
  'copilot-cli': { bg: '#24292E', mark: 'gh' },
  // Detection-only harnesses — monogram tiles until brand SVGs ship.
  'junie-cli': { bg: '#F97316', mark: 'J' },
  droid: { bg: '#7C3AED', mark: 'Dr' },
  'kimi-code-cli': { bg: '#1F2937', mark: 'K' },
  devin: { bg: '#0891B2', mark: 'Dv' },
  forgecode: { bg: '#B45309', mark: 'Fg' },
  sentinel: { bg: '#4F46E5', mark: 'Sn' },
};

export type BackendKind = Backend['kind'];

export const BACKEND_LABELS: Record<BackendKind, string> = {
  signoz: 'SigNoz',
  honeycomb: 'Honeycomb',
  'grafana-cloud': 'Grafana',
  datadog: 'Datadog',
  'otlp-generic': 'Generic OTLP',
  'otelcol-passthrough': 'Local Collector',
  'new-relic': 'New Relic',
  'splunk-observability': 'Splunk',
  dynatrace: 'Dynatrace',
  elastic: 'Elastic',
  opensearch: 'OpenSearch',
  openobserve: 'OpenObserve',
  clickstack: 'ClickStack',
  chronosphere: 'Chronosphere',
  sentry: 'Sentry',
};

export const BACKEND_LOGOS: Record<BackendKind, LogoFallback> = {
  signoz: { bg: '#E75A50', mark: 'S' },
  honeycomb: { bg: '#F5A623', mark: 'H' },
  'grafana-cloud': { bg: '#F46800', mark: 'G' },
  datadog: { bg: '#632CA6', mark: 'D' },
  'otlp-generic': { bg: '#425CC7', mark: 'O' },
  'otelcol-passthrough': { bg: '#F2A33A', mark: 'OC' },
  'new-relic': { bg: '#1CE783', mark: 'NR' },
  'splunk-observability': { bg: '#FF375F', mark: 'Sp' },
  dynatrace: { bg: '#1496FF', mark: 'Dt' },
  elastic: { bg: '#00BFB3', mark: 'El' },
  opensearch: { bg: '#005EB8', mark: 'Os' },
  openobserve: { bg: '#FFC107', mark: 'Oo' },
  clickstack: { bg: '#FAFF69', mark: 'Cs' },
  chronosphere: { bg: '#2D5BFF', mark: 'Cr' },
  sentry: { bg: '#6C5FC7', mark: 'Sn' },
};

interface ParsedBrandSvg {
  type: 'svg';
  viewBox: string;
  inner: string;
}

interface BrandPng {
  type: 'png';
  url: string;
}

export type BrandArtwork = ParsedBrandSvg | BrandPng;

const HARNESS_BRAND_SVG_SOURCES = import.meta.glob<string>('../assets/harness-logos/*.svg', {
  eager: true,
  query: '?raw',
  import: 'default',
});
const HARNESS_BRAND_PNG_SOURCES = import.meta.glob<string>('../assets/harness-logos/*.png', {
  eager: true,
  query: '?url',
  import: 'default',
});
const BACKEND_BRAND_SVG_SOURCES = import.meta.glob<string>('../assets/backend-logos/*.svg', {
  eager: true,
  query: '?raw',
  import: 'default',
});
const BACKEND_BRAND_PNG_SOURCES = import.meta.glob<string>('../assets/backend-logos/*.png', {
  eager: true,
  query: '?url',
  import: 'default',
});

const PARSED_HARNESS_LOGOS = new Map<string, BrandArtwork | null>();
for (const [path, raw] of Object.entries(HARNESS_BRAND_SVG_SOURCES)) {
  PARSED_HARNESS_LOGOS.set(path, parseBrandSvg(raw));
}
for (const [path, url] of Object.entries(HARNESS_BRAND_PNG_SOURCES)) {
  PARSED_HARNESS_LOGOS.set(path, { type: 'png', url });
}
const PARSED_BACKEND_LOGOS = new Map<string, BrandArtwork | null>();
for (const [path, raw] of Object.entries(BACKEND_BRAND_SVG_SOURCES)) {
  PARSED_BACKEND_LOGOS.set(path, parseBrandSvg(raw));
}
for (const [path, url] of Object.entries(BACKEND_BRAND_PNG_SOURCES)) {
  PARSED_BACKEND_LOGOS.set(path, { type: 'png', url });
}

function parseBrandSvg(raw: string): ParsedBrandSvg | null {
  const open = raw.match(/<svg\b([^>]*)>/i);
  const close = raw.lastIndexOf('</svg>');
  if (!open || open.index === undefined || close === -1) return null;
  const openAttrs = open[1] ?? '';
  const viewBoxMatch = openAttrs.match(/viewBox\s*=\s*"([^"]+)"/i);
  const viewBox = viewBoxMatch?.[1];
  if (!viewBox) return null;
  // Preserve any `fill="..."` on the root <svg> so brand colors propagate
  // when we re-render the inner contents under a fresh <svg> element.
  // Without this, an SVG with the fill declared once at the root would
  // render as black when nested.
  const fillMatch = openAttrs.match(/fill\s*=\s*"([^"]+)"/i);
  const rootFill = fillMatch?.[1];
  const innerRaw = raw.slice(open.index + open[0].length, close).trim();
  const inner = rootFill ? `<g fill="${rootFill}">${innerRaw}</g>` : innerRaw;
  return { type: 'svg', viewBox, inner };
}

export function harnessBrandLogo(id: HarnessId): BrandArtwork | undefined {
  // Prefer SVG when both are present; PNG is the fallback for harnesses
  // (like Aider) whose brand asset is only available as a raster.
  return (
    PARSED_HARNESS_LOGOS.get(`../assets/harness-logos/${id}.svg`) ??
    PARSED_HARNESS_LOGOS.get(`../assets/harness-logos/${id}.png`) ??
    undefined
  );
}

export function backendBrandLogo(kind: BackendKind): BrandArtwork | undefined {
  // Prefer SVG (already keyed by .svg path) when both are present; the
  // PNG entry under the .png path is checked as a fallback.
  return (
    PARSED_BACKEND_LOGOS.get(`../assets/backend-logos/${kind}.svg`) ??
    PARSED_BACKEND_LOGOS.get(`../assets/backend-logos/${kind}.png`) ??
    undefined
  );
}

interface HarnessLogoProps {
  id: HarnessId;
  /** Dim + desaturate. Used for undetected rows. */
  dimmed?: boolean;
  /** Rendered pixel size. Default 32. */
  size?: number;
  /** Override the testid (default `harness-logo-{id}`). */
  testid?: string;
}

/** Standalone HTML `<svg>` element for harness rows / chips. Prefers
 *  real brand artwork from `src/assets/harness-logos/*.svg`; falls back
 *  to a monogram tile. */
export function HarnessLogo({
  id,
  dimmed = false,
  size = 32,
  testid,
}: HarnessLogoProps): JSX.Element {
  const className = `shrink-0 ${dimmed ? 'opacity-40 grayscale' : ''}`;
  const tid = testid ?? `harness-logo-${id}`;
  const brand = harnessBrandLogo(id);
  if (brand) {
    return renderBrandArtwork(brand, {
      size,
      className,
      testid: tid,
      ariaLabel: `${HARNESS_LABELS[id]} logo`,
    });
  }
  return (
    <MonogramSvg
      fallback={HARNESS_LOGOS[id]}
      size={size}
      className={className}
      testid={tid}
      ariaLabel={`${HARNESS_LABELS[id]} logo`}
    />
  );
}

interface BackendLogoProps {
  kind: BackendKind;
  dimmed?: boolean;
  size?: number;
  testid?: string;
}

/** Standalone HTML element for forwarding-backend rows. Prefers real
 *  brand artwork (SVG or PNG) from `src/assets/backend-logos/`; falls
 *  back to a monogram tile when none is available. */
export function BackendLogo({
  kind,
  dimmed = false,
  size = 32,
  testid,
}: BackendLogoProps): JSX.Element {
  const className = `shrink-0 ${dimmed ? 'opacity-40 grayscale' : ''}`;
  const tid = testid ?? `backend-logo-${kind}`;
  const brand = backendBrandLogo(kind);
  if (brand) {
    return renderBrandArtwork(brand, {
      size,
      className,
      testid: tid,
      ariaLabel: `${BACKEND_LABELS[kind]} logo`,
    });
  }
  return (
    <MonogramSvg
      fallback={BACKEND_LOGOS[kind]}
      size={size}
      className={className}
      testid={tid}
      ariaLabel={`${BACKEND_LABELS[kind]} logo`}
    />
  );
}

interface RenderBrandOptions {
  size: number;
  className: string;
  testid: string;
  ariaLabel: string;
}

function renderBrandArtwork(art: BrandArtwork, opts: RenderBrandOptions): JSX.Element {
  if (art.type === 'svg') {
    return (
      <svg
        width={opts.size}
        height={opts.size}
        viewBox={art.viewBox}
        xmlns="http://www.w3.org/2000/svg"
        role="img"
        aria-label={opts.ariaLabel}
        data-testid={opts.testid}
        className={opts.className}
        dangerouslySetInnerHTML={{ __html: art.inner }}
      />
    );
  }
  return (
    <img
      src={art.url}
      width={opts.size}
      height={opts.size}
      alt={opts.ariaLabel}
      data-testid={opts.testid}
      className={`${opts.className} rounded-[6px] object-contain`}
    />
  );
}

interface MonogramSvgProps {
  fallback: LogoFallback;
  size: number;
  className: string;
  testid: string;
  ariaLabel: string;
}

function MonogramSvg({
  fallback,
  size,
  className,
  testid,
  ariaLabel,
}: MonogramSvgProps): JSX.Element {
  const { bg, mark } = fallback;
  const fontSize = mark.length >= 2 ? size * 0.34 : size * 0.44;
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 32 32"
      xmlns="http://www.w3.org/2000/svg"
      role="img"
      aria-label={ariaLabel}
      data-testid={testid}
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

interface NodeLogoSvgProps {
  /** Top-left x coordinate in the parent SVG's user space. */
  x: number;
  /** Top-left y coordinate. */
  y: number;
  /** Box size (width and height — square). */
  size: number;
  /** Harness id (preferred — brand SVG when available, else monogram). */
  harnessId?: HarnessId;
  /** Backend kind (monogram fallback only — no brand SVGs yet). */
  backendKind?: BackendKind;
}

/** Embeddable logo for use INSIDE another `<svg>` element (e.g. the
 *  FlowChart). Renders a nested `<svg>` for vector artwork, or an
 *  SVG `<image>` element for PNG artwork. */
export function NodeLogoSvg({
  x,
  y,
  size,
  harnessId,
  backendKind,
}: NodeLogoSvgProps): JSX.Element | null {
  if (harnessId) {
    const brand = harnessBrandLogo(harnessId);
    if (brand) return renderBrandInsideSvg(brand, x, y, size);
    return <MonogramG x={x} y={y} size={size} fallback={HARNESS_LOGOS[harnessId]} />;
  }
  if (backendKind) {
    const brand = backendBrandLogo(backendKind);
    if (brand) return renderBrandInsideSvg(brand, x, y, size);
    return <MonogramG x={x} y={y} size={size} fallback={BACKEND_LOGOS[backendKind]} />;
  }
  return null;
}

function renderBrandInsideSvg(art: BrandArtwork, x: number, y: number, size: number): JSX.Element {
  if (art.type === 'svg') {
    return (
      <svg
        x={x}
        y={y}
        width={size}
        height={size}
        viewBox={art.viewBox}
        dangerouslySetInnerHTML={{ __html: art.inner }}
      />
    );
  }
  // SVG `<image>` for raster artwork. preserveAspectRatio keeps the
  // logo from being squashed if the host node aspect changes.
  return (
    <image
      x={x}
      y={y}
      width={size}
      height={size}
      href={art.url}
      preserveAspectRatio="xMidYMid meet"
    />
  );
}

function MonogramG({
  x,
  y,
  size,
  fallback,
}: {
  x: number;
  y: number;
  size: number;
  fallback: LogoFallback;
}): JSX.Element {
  const { bg, mark } = fallback;
  const fontSize = mark.length >= 2 ? size * 0.34 : size * 0.46;
  const cx = x + size / 2;
  const cy = y + size / 2;
  const radius = size * 0.25;
  return (
    <g>
      <rect x={x} y={y} width={size} height={size} rx={radius} ry={radius} fill={bg} />
      <text
        x={cx}
        y={cy}
        textAnchor="middle"
        dominantBaseline="central"
        fontFamily="system-ui, -apple-system, sans-serif"
        fontSize={fontSize}
        fontWeight={700}
        fill="#ffffff"
      >
        {mark}
      </text>
    </g>
  );
}

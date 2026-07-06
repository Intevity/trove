// Single source of truth for the homepage feature carousel. Shared by the React
// carousel island and any static grid that renders the same cards, so the data
// shape here is the contract the section components read. Copy is written fresh
// from current Trove behaviour (see README.md / documentation/*) and kept free
// of em dashes per the project's UI-copy rule.
//
// Demo videos are recorded (via Curtain live capture; see .curtain/capture) and
// live at public/videos/<slug>.{mp4,webm} with a JPG poster frame at <slug>.jpg,
// resolved at render time via import.meta.env.BASE_URL. hasVideo is true for every
// entry; the carousel renders the <video> and falls back to the poster only if a
// clip is ever removed.

export type IconName = 'radar' | 'toggle' | 'share' | 'chart' | 'heart-pulse' | 'git-merge';

export interface Feature {
  slug: string;
  /** Short tab label used in the carousel rail. */
  label: string;
  /** Card / panel heading. */
  title: string;
  /** One-line hook shown under the title. */
  tagline: string;
  /** One to two sentences for the carousel panel and any grid card. */
  description: string;
  icon: IconName;
  /** File in public/videos/. Set hasVideo true once the real clip is recorded. */
  video: string;
  /** SVG placeholder in public/videos/, resolved via BASE_URL. */
  poster: string;
  /** Teal-family accent (CSS color) for the card glow / icon chip. */
  accent: string;
  /** Flip to true after dropping the real recording into public/videos/. */
  hasVideo: boolean;
}

export const features: Feature[] = [
  {
    slug: 'detect',
    label: 'Detect',
    title: 'Auto-detect every harness',
    tagline: 'One launch sweeps every standard install path on your machine.',
    description:
      'Trove scans for all 17 supported AI coding tools the moment it starts and surfaces exactly what is on disk. No manual inventory, no config spelunking, just the list of harnesses you actually have.',
    icon: 'radar',
    video: 'detect.mp4',
    poster: 'detect.jpg',
    accent: '#2dbfb8',
    hasVideo: true,
  },
  {
    slug: 'enable',
    label: 'Enable',
    title: 'Enable telemetry in one click',
    tagline: 'Toggle a row and Trove patches that tool to emit OTLP.',
    description:
      'Each Enable writes a sentinel-bracketed managed region into the harness config and the row turns green the instant OTLP starts flowing through the local collector. One click reverts it byte-for-byte, with no orphaned env vars left behind.',
    icon: 'toggle',
    video: 'enable.mp4',
    poster: 'enable.jpg',
    accent: '#2dbfb8',
    hasVideo: true,
  },
  {
    slug: 'fan-out',
    label: 'Fan out',
    title: 'Fan out to any backend',
    tagline: 'Every signal broadcasts to every backend you enable.',
    description:
      'Configure SigNoz, Honeycomb, Datadog, or any of the 15 supported backends, and Trove forwards one unified stream to all of them at once. The collector binds to 127.0.0.1 and sends only to the endpoints you set, never to Trove.',
    icon: 'share',
    video: 'fan-out.mp4',
    poster: 'fan-out.jpg',
    accent: '#26a8a2',
    hasVideo: true,
  },
  {
    slug: 'metrics',
    label: 'Tier A',
    title: 'Compare tools on one metric scale (Tier A)',
    tagline: 'A normalized cross-harness schema for true side-by-side cost.',
    description:
      'Tier A metrics (trove.harness.events, tokens, cost.usd, turn.duration, errors) give every harness one consistent shape. Cost per turn for Claude Code is directly comparable to Copilot CLI in your own dashboard, with no vendor-specific exporter to maintain.',
    icon: 'chart',
    video: 'metrics.mp4',
    poster: 'metrics.jpg',
    accent: '#2dbfb8',
    hasVideo: true,
  },
  {
    slug: 'health',
    label: 'Health',
    title: 'See backend health at a glance',
    tagline: 'A 4-color pill per backend, driven by the collector itself.',
    description:
      'Each configured platform carries a green / amber / red / gray health pill driven by the collector scrape metrics, so you see the moment an exporter starts dropping traffic instead of waiting for a dashboard to go quiet.',
    icon: 'heart-pulse',
    video: 'health.mp4',
    poster: 'health.jpg',
    accent: '#26a8a2',
    hasVideo: true,
  },
  {
    slug: 'mappings',
    label: 'Mappings',
    title: 'Tune the signal mapping visually',
    tagline: 'A visual editor from native signal to Tier A, with a live diff.',
    description:
      'The Mappings tab is a visual editor that maps each native signal onto a Tier A metric and shows a live diff as you edit. Synthesis rules cover native OTel harnesses; hook rules classify watcher events for the best-effort ones.',
    icon: 'git-merge',
    video: 'mappings.mp4',
    poster: 'mappings.jpg',
    accent: '#2dbfb8',
    hasVideo: true,
  },
];

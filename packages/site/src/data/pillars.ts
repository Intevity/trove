// Deep-dive sections for the four narratives that sell Trove: one pane of glass
// for mixed-harness teams, cross-vendor cost normalization, private-by-design
// architecture, and native-vs-best-effort coverage. The flat carousel
// (features.ts) is the "see it in action" overview; this is the marketing
// narrative that breaks each pillar into the sub-features users came for. Copy is
// grounded in README.md / documentation/* and kept free of em dashes per the
// project's UI-copy rule.
//
// Recorded clips live at public/videos/<slug>.{mp4,webm} with a JPG poster frame
// at <slug>.jpg (via Curtain live capture; see .curtain/capture), resolved with
// import.meta.env.BASE_URL by the section component. hasVideo is true for those.
// The one exception is `keychain`, which has no recording yet: it stays hasVideo
// false with an SVG placeholder poster.

export interface SubFeature {
  /** Icon name resolved by Icon.astro (must exist in its path map). */
  icon: string;
  title: string;
  /** One short phrase under the title. */
  hook: string;
  description: string;
  /** File in public/videos/. Flip hasVideo true once recorded. */
  video: string;
  /** SVG placeholder in public/videos/, resolved via BASE_URL. */
  poster: string;
  hasVideo: boolean;
}

export interface Clip {
  slug: string;
  title: string;
  /** SVG placeholder in public/videos/, resolved via BASE_URL. */
  poster: string;
  hasVideo: boolean;
}

export interface Pillar {
  slug: string;
  /** Short kicker above the pillar title. */
  eyebrow: string;
  title: string;
  intro: string;
  /** Icon name resolved by Icon.astro (must exist in its path map). */
  icon: string;
  subFeatures: SubFeature[];
  /** Demo clips for this pillar's media rail (poster-only for now). */
  clips: Clip[];
}

export const pillars: Pillar[] = [
  {
    slug: 'one-pane',
    eyebrow: 'For engineering leads',
    title: 'One pane of glass for mixed-harness teams',
    intro:
      'Half the team is on Claude Code, half on Cursor plus Copilot CLI, and someone trialed Codex last week. Every span, metric, and log Trove forwards carries a harness.id resource attribute, so all of it lands in one backend you can query side by side.',
    icon: 'network',
    subFeatures: [
      {
        icon: 'radar',
        title: 'Auto-detected on launch',
        hook: 'Every harness on the machine, surfaced',
        description:
          'Trove sweeps the standard install paths the moment it starts and lists exactly which of the 17 supported tools are present, so the inventory is never guesswork.',
        video: 'overview.mp4',
        poster: 'overview.jpg',
        hasVideo: true,
      },
      {
        icon: 'activity',
        title: 'Live data-flow chart',
        hook: 'Watch telemetry flow in real time',
        description:
          'The Overview tab animates each source through the collector to your backends. Three or fewer tools render as individual nodes; four or more collapse into an animated Orbital Hub cluster with per-source activity halos.',
        video: 'flow-chart.mp4',
        poster: 'flow-chart.jpg',
        hasVideo: true,
      },
    ],
    clips: [
      {
        slug: 'overview',
        title: 'Overview health and data flow',
        poster: 'overview.jpg',
        hasVideo: true,
      },
      {
        slug: 'flow-chart',
        title: 'Orbital Hub data-flow chart',
        poster: 'flow-chart.jpg',
        hasVideo: true,
      },
    ],
  },
  {
    slug: 'cost-normalization',
    eyebrow: 'For finance and ops',
    title: 'Cross-vendor cost normalization and dead-seat detection',
    intro:
      'You pay for Cursor, Copilot, Claude Code, and Codex seats across the org. Trove routes every tool through the same Tier A metric schema, so cost per turn is comparable across vendors and zero-activity seats surface before renewal.',
    icon: 'chart',
    subFeatures: [
      {
        icon: 'chart',
        title: 'One cost scale, every tool',
        hook: 'Tier A makes vendors comparable',
        description:
          'Token counts, model-call counts, and turn durations all flow through trove.harness.tokens, events, cost.usd, and turn.duration. Cost per turn for Claude Code lines up directly against Copilot CLI in your own dashboard.',
        video: 'cost-normalization.mp4',
        poster: 'cost-normalization.jpg',
        hasVideo: true,
      },
      {
        icon: 'users',
        title: 'Dead-seat detection',
        hook: 'Find paid seats with zero activity',
        description:
          'The same harness.id-keyed metric stream surfaces which licenses fire and which are silent, by user and by week, so procurement can reclaim seats with zero turns in the last 30 days.',
        video: 'dead-seats.mp4',
        poster: 'dead-seats.jpg',
        hasVideo: true,
      },
    ],
    clips: [
      {
        slug: 'cost-normalization',
        title: 'Tier A cost normalization',
        poster: 'cost-normalization.jpg',
        hasVideo: true,
      },
      {
        slug: 'dead-seats',
        title: 'Dead-seat detection',
        poster: 'dead-seats.jpg',
        hasVideo: true,
      },
    ],
  },
  {
    slug: 'private-by-design',
    eyebrow: 'For security and IT',
    title: 'Private by architecture, reversible by design',
    intro:
      'Trove never phones home. The collector binds to localhost and forwards only to the backend you set, credentials live in the OS keychain, and every config patch is a managed region you can revert byte-for-byte. It is all MIT-licensed and auditable.',
    icon: 'shield-check',
    subFeatures: [
      {
        icon: 'eye-off',
        title: 'Localhost-only',
        hook: 'Binds 127.0.0.1, forwards only to you',
        description:
          'The bundled OpenTelemetry Collector listens on 127.0.0.1 and exports exclusively to the endpoint you configured. No analytics, no crash reporting, no third-party SDK phoning a vendor.',
        video: 'localhost-only.mp4',
        poster: 'localhost-only.jpg',
        hasVideo: true,
      },
      {
        icon: 'key',
        title: 'Credentials in the OS keychain',
        hook: 'Keychain, Credential Manager, Secret Service',
        description:
          'Backend tokens and ingest secrets live in macOS Keychain, Windows Credential Manager, or Linux Secret Service. Never in plaintext JSON, never in env files, never logged.',
        video: 'keychain.mp4',
        poster: 'keychain.svg',
        hasVideo: false,
      },
      {
        icon: 'rotate-ccw',
        title: 'Reversible, auditable patches',
        hook: 'Every change reverts byte-for-byte',
        description:
          'Each Enable wraps the harness config in a sentinel-bracketed managed region you can diff, audit, and revert in one click. What got written is captured in a single versioned state.json.',
        video: 'reversible-revert.mp4',
        poster: 'reversible-revert.jpg',
        hasVideo: true,
      },
    ],
    clips: [
      {
        slug: 'localhost-only',
        title: 'Localhost-only forwarding',
        poster: 'localhost-only.jpg',
        hasVideo: true,
      },
      {
        slug: 'reversible-revert',
        title: 'Byte-for-byte revert',
        poster: 'reversible-revert.jpg',
        hasVideo: true,
      },
    ],
  },
  {
    slug: 'coverage',
    eyebrow: 'How coverage works',
    title: 'Native OpenTelemetry where it exists, best-effort everywhere else',
    intro:
      'Some harnesses speak OTLP natively; Trove just flips the right flags. The ones that do not get lightweight watchers and shell-rc wrappers that derive equivalent OTLP records, so the derived signals answer the same queries as their native peers.',
    icon: 'layers',
    subFeatures: [
      {
        icon: 'box',
        title: 'Native OTel passthrough',
        hook: 'Flip the flag, route the stream',
        description:
          'Claude Code, Antigravity CLI, Codex, Qwen, OpenCode, and Cursor IDE emit OTLP natively. Trove sets the right env and config flags and routes their signals straight through the collector.',
        video: 'native-otel.mp4',
        poster: 'native-otel.jpg',
        hasVideo: true,
      },
      {
        icon: 'git-merge',
        title: 'Best-effort adapters',
        hook: 'Watchers derive OTLP from logs',
        description:
          'Cline and other harnesses that do not emit OTel natively get Trove’s filesystem watchers and shell-rc wrappers, which derive OTLP records from their on-disk logs into the same Tier A shape.',
        video: 'best-effort-adapter.mp4',
        poster: 'best-effort-adapter.jpg',
        hasVideo: true,
      },
    ],
    clips: [
      {
        slug: 'native-otel',
        title: 'Native OTel passthrough',
        poster: 'native-otel.jpg',
        hasVideo: true,
      },
      {
        slug: 'best-effort-adapter',
        title: 'Best-effort watcher adapter',
        poster: 'best-effort-adapter.jpg',
        hasVideo: true,
      },
    ],
  },
];

/**
 * Trove collector presets — TS metadata layer.
 *
 * The actual YAML templates live as checked-in files under
 * `templates/` and are consumed by the Rust codegen module via
 * `include_str!`. Keeping them as plain YAML files (not template
 * strings inlined into TS) means they diff cleanly when we sync with
 * upstream vendor docs, and golden-file tests can snapshot them as-is.
 *
 * This module exposes the metadata the React wizard renders — preset
 * kind, human-readable label, one-line description, and the
 * "Recommended" flag that pins SigNoz to the top of the picker (see
 * MVP_PLAN §"Resolved design decisions").
 */

import type { BackendDraft } from '@trove/shared';

/** The discriminator literal of {@link BackendDraft}. */
export type PresetKind = BackendDraft['kind'];

export interface PresetMetadata {
  kind: PresetKind;
  label: string;
  description: string;
  /** True for the wizard's default selection (SigNoz). The wizard
   *  surfaces this by sort order and a "Recommended" pill. */
  recommended: boolean;
}

/** Ordered list shown by the wizard's preset picker. SigNoz first. */
export const PRESETS: readonly PresetMetadata[] = [
  {
    kind: 'signoz',
    label: 'SigNoz Cloud',
    description:
      'Open-source observability platform. Recommended for most users — works out of the box with native OTLP and a built-in dashboard.',
    recommended: true,
  },
  {
    kind: 'honeycomb',
    label: 'Honeycomb',
    description:
      'High-cardinality observability for distributed systems. Sends to https://api.honeycomb.io with your team API key.',
    recommended: false,
  },
  {
    kind: 'grafana-cloud',
    label: 'Grafana',
    description:
      "Forwards to your Grafana stack's OTLP gateway. Copy the endpoint and Basic auth credentials from your stack onboarding page.",
    recommended: false,
  },
  {
    kind: 'datadog',
    label: 'Datadog',
    description:
      'Sends to Datadog’s OTLP intake using a DD-API-KEY header. The site selector picks between datadoghq.com, datadoghq.eu, and other regional intakes.',
    recommended: false,
  },
  {
    kind: 'otlp-generic',
    label: 'Generic OTLP',
    description:
      'Any backend that accepts OTLP. Provide an endpoint, choose gRPC or HTTP, and add as many auth headers as you need.',
    recommended: false,
  },
  {
    kind: 'otelcol-passthrough',
    label: 'Local Collector (passthrough)',
    description:
      'For users already running their own OpenTelemetry Collector. Trove forwards harness telemetry to your endpoint without adding credentials.',
    recommended: false,
  },
  {
    kind: 'new-relic',
    label: 'New Relic',
    description:
      'Forwards to New Relic’s OTLP/HTTP intake using your license key in an `api-key` header. Pick US or EU to select the regional cluster.',
    recommended: false,
  },
  {
    kind: 'splunk-observability',
    label: 'Splunk Observability Cloud',
    description:
      'OTLP/HTTP to Splunk Observability Cloud. The realm (e.g. us1, eu0) shapes the ingest hostname; the access token authenticates via the `X-SF-Token` header.',
    recommended: false,
  },
  {
    kind: 'dynatrace',
    label: 'Dynatrace',
    description:
      'OTLP/HTTP to a Dynatrace environment. Trove appends `/api/v2/otlp` to the environment URL and authenticates with `Authorization: Api-Token <token>`.',
    recommended: false,
  },
  {
    kind: 'elastic',
    label: 'Elastic',
    description:
      'OTLP/HTTP to an Elastic APM Server endpoint. Authenticates via `Authorization: ApiKey <key>`. Works with Elastic Cloud and self-managed deployments.',
    recommended: false,
  },
  {
    kind: 'opensearch',
    label: 'OpenSearch',
    description:
      'OTLP/HTTP to an OpenSearch ingest endpoint (typically Data Prepper). Authentication is whatever you paste into the Authorization header — usually a `Basic` token.',
    recommended: false,
  },
  {
    kind: 'openobserve',
    label: 'OpenObserve',
    description:
      'OTLP/HTTP to OpenObserve. Trove appends `/api/<organization>` to your endpoint and forwards the Authorization header as-is (paste the `Basic` value from OpenObserve).',
    recommended: false,
  },
  {
    kind: 'clickstack',
    label: 'ClickStack (HyperDX)',
    description:
      'OTLP/HTTP to ClickStack / HyperDX. The ingestion key goes into a single `authorization` header; the endpoint is whatever your deployment exposes (default: in-otel.hyperdx.io).',
    recommended: false,
  },
  {
    kind: 'chronosphere',
    label: 'Chronosphere',
    description:
      'OTLP/HTTP to your Chronosphere tenant. Trove builds `https://<tenant>.chronosphere.io/data/v1/otlp` and authenticates with the `API-Token` header.',
    recommended: false,
  },
  {
    kind: 'sentry',
    label: 'Sentry',
    description:
      'OTLP/HTTP to Sentry’s OTLP intake. Paste the full ingest URL and the `X-Sentry-Auth` header value from your project settings.',
    recommended: false,
  },
];

/** Fast lookup by kind. */
export function presetMetadataFor(kind: PresetKind): PresetMetadata {
  const found = PRESETS.find((p) => p.kind === kind);
  if (!found) {
    // The PRESETS array is hand-maintained against the BackendDraft
    // discriminated union — TypeScript covers compile-time exhaustivity
    // when adding a kind, but we throw at runtime as a backstop in case
    // a malformed payload reaches us through the IPC boundary.
    throw new Error(`unknown preset kind: ${kind as string}`);
  }
  return found;
}

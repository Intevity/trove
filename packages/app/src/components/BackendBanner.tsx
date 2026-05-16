import { presetMetadataFor } from '@trove/collector-presets';
import type { Backend } from '@trove/shared';

import { Button } from './ui/index.js';

export interface BackendBannerProps {
  backend: Backend;
  onChange: () => void;
}

/** Slim macOS-native banner showing the configured backend and a
 *  ghost "Change" affordance. Sits between Diagnostics and the
 *  Collector card on the Overview tab. */
export function BackendBanner({ backend, onChange }: BackendBannerProps): JSX.Element {
  const meta = presetMetadataFor(backend.kind);
  return (
    <div
      data-testid="backend-banner"
      className="flex items-center justify-between gap-2 rounded-card border border-hairline bg-surface-elevated px-3 py-2 dark:border-hairline-dark dark:bg-surface-elevated-dark"
    >
      <span className="text-[12px] text-fg-secondary dark:text-fg-secondary-dark">
        Forwarding to{' '}
        <span className="font-medium text-fg-primary dark:text-fg-primary-dark">{meta.label}</span>
        <BackendDetail backend={backend} />
      </span>
      <Button variant="ghost" size="sm" testid="backend-banner-change" onClick={onChange}>
        Change
      </Button>
    </div>
  );
}

function BackendDetail({ backend }: { backend: Backend }): JSX.Element | null {
  switch (backend.kind) {
    case 'signoz':
      return (
        <span className="text-fg-tertiary dark:text-fg-tertiary-dark"> ({backend.endpoint})</span>
      );
    case 'honeycomb':
      return (
        <span className="text-fg-tertiary dark:text-fg-tertiary-dark"> ({backend.dataset})</span>
      );
    case 'datadog':
      return <span className="text-fg-tertiary dark:text-fg-tertiary-dark"> ({backend.site})</span>;
    case 'grafana-cloud':
    case 'otlp-generic':
    case 'otelcol-passthrough':
    case 'new-relic':
    case 'splunk-observability':
    case 'dynatrace':
    case 'elastic':
    case 'opensearch':
    case 'openobserve':
    case 'clickstack':
    case 'chronosphere':
    case 'sentry':
      return null;
  }
}

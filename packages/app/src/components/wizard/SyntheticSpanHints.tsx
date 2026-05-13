import { useState } from 'react';

import { presetMetadataFor } from '@trove/collector-presets';
import type { BackendDraft } from '@trove/shared';
import { SYNTHETIC_SERVICE_NAME, SYNTHETIC_SPAN_NAME, SYNTHETIC_TRACE_ID } from '@trove/shared';

interface Props {
  backendKind?: BackendDraft['kind'] | undefined;
}

export function SyntheticSpanHints({ backendKind }: Props): JSX.Element {
  const backendLabel = backendKind ? presetMetadataFor(backendKind).label : null;
  const preamble = backendLabel
    ? `Look for this in ${backendLabel}:`
    : 'Look for this in your observability backend:';

  return (
    <div
      data-testid="synthetic-span-hints"
      className="mt-2 rounded-card border border-ios-green/25 bg-ios-green/[0.08] px-3 py-2 text-[12px] text-fg-primary dark:text-fg-primary-dark"
    >
      <p className="font-medium">{preamble}</p>
      <dl className="mt-1.5 grid grid-cols-[max-content_1fr_auto] items-center gap-x-3 gap-y-1">
        <HintRow label="service.name" value={SYNTHETIC_SERVICE_NAME} testid="hint-service-name" />
        <HintRow label="span name" value={SYNTHETIC_SPAN_NAME} testid="hint-span-name" />
        <HintRow
          label="trace id"
          value={SYNTHETIC_TRACE_ID}
          testid="hint-trace-id"
          title="Fixed canary id. Each test export reuses the same trace id so successive runs overwrite the same trace at the backend."
        />
      </dl>
    </div>
  );
}

function HintRow({
  label,
  value,
  testid,
  title,
}: {
  label: string;
  value: string;
  testid: string;
  title?: string;
}): JSX.Element {
  return (
    <>
      <dt className="font-mono text-[11px] text-ios-green">{label}</dt>
      <dd
        className="overflow-hidden truncate font-mono text-[11px] text-fg-primary dark:text-fg-primary-dark"
        data-testid={testid}
        title={title ?? value}
      >
        {value}
      </dd>
      <CopyButton value={value} testid={`${testid}-copy`} />
    </>
  );
}

function CopyButton({ value, testid }: { value: string; testid: string }): JSX.Element {
  const [copied, setCopied] = useState(false);
  const handleClick = (): void => {
    void copyToClipboard(value).then((ok) => {
      if (!ok) return;
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1200);
    });
  };
  return (
    <button
      type="button"
      onClick={handleClick}
      data-testid={testid}
      className="rounded-[6px] border border-ios-green/40 bg-surface px-1.5 py-0.5 text-[10px] font-medium text-ios-green hover:bg-ios-green/[0.10] dark:bg-surface-dark"
      aria-label={`copy ${value}`}
    >
      {copied ? 'copied' : 'copy'}
    </button>
  );
}

async function copyToClipboard(value: string): Promise<boolean> {
  if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(value);
      return true;
    } catch {
      // fall through to noop; clipboard can fail in non-secure contexts
    }
  }
  return false;
}

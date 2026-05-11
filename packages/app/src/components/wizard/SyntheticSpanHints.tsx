import { useState } from 'react';

import { presetMetadataFor } from '@trove/collector-presets';
import type { BackendDraft } from '@trove/shared';
import { SYNTHETIC_SERVICE_NAME, SYNTHETIC_SPAN_NAME, SYNTHETIC_TRACE_ID } from '@trove/shared';

interface Props {
  /** Backend label key for the "Look for this in {label}" preamble. When
   *  unknown (wizard pre-credential step, dashboard without a chosen
   *  backend), the component falls back to a generic phrasing. */
  backendKind?: BackendDraft['kind'] | undefined;
}

/** Post-success guidance shown after a synthetic OTLP trace lands. The
 *  Rust IPC sends a canary trace with fixed identifiers — see
 *  `packages/app/src-tauri/src/ipc/test_export.rs`. The user needs to
 *  know which `service.name` / span name / trace ID to search for in
 *  their observability tool; this component surfaces them with copy
 *  buttons so the values can be pasted into the backend's UI verbatim.
 *
 *  Kept presentational and prop-driven so the same component renders in
 *  the wizard's `TestExportStep` and the dashboard's `SidecarPanel`. */
export function SyntheticSpanHints({ backendKind }: Props): JSX.Element {
  const backendLabel = backendKind ? presetMetadataFor(backendKind).label : null;
  const preamble = backendLabel
    ? `Look for this in ${backendLabel}:`
    : 'Look for this in your observability backend:';

  return (
    <div
      data-testid="synthetic-span-hints"
      className="mt-2 rounded-md border border-emerald-200 bg-emerald-50/60 px-3 py-2 text-xs text-emerald-900 dark:border-emerald-800 dark:bg-emerald-950/40 dark:text-emerald-200"
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
      <dt className="font-mono text-[11px] text-emerald-700 dark:text-emerald-300">{label}</dt>
      <dd
        className="overflow-hidden truncate font-mono text-[11px]"
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
      className="rounded border border-emerald-300 bg-white px-1.5 py-0.5 text-[10px] font-medium text-emerald-800 hover:bg-emerald-100 dark:border-emerald-700 dark:bg-slate-900 dark:text-emerald-200 dark:hover:bg-emerald-900/30"
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

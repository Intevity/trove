import { useState, type JSX } from 'react';

import { type HarnessId, type MappingState, type SimulateMappingResponse } from '@trove/shared';

import { Button, Sheet } from '../../ui/index.js';
import { simulateMapping, TroveIpcError } from '../../../lib/ipc.js';

interface Props {
  open: boolean;
  onClose: () => void;
  draftState: MappingState;
  harnessId: HarnessId;
  /** Index into the harness's `sources` array — the rule the user opened
   *  Preview from. */
  sourceIndex: number;
}

/** Dry-run preview dialog: applies one rule's transform to a sample
 *  input the user enters, and shows the post-transform output side-by-
 *  side with any validation notes. Calls the `simulate_mapping` IPC. */
export function MappingPreviewSheet({
  open,
  onClose,
  draftState,
  harnessId,
  sourceIndex,
}: Props): JSX.Element {
  const [sampleAttrs, setSampleAttrs] = useState<Record<string, string>>({});
  const [sampleValue, setSampleValue] = useState('1');
  const [pendingKey, setPendingKey] = useState('');
  const [pendingValue, setPendingValue] = useState('');
  const [result, setResult] = useState<SimulateMappingResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const sourceRule = draftState.harnesses.find((h) => h.harnessId === harnessId)?.sources[
    sourceIndex
  ];

  const runPreview = async () => {
    setBusy(true);
    setError(null);
    setResult(null);
    try {
      const out = await simulateMapping({
        mappingState: draftState,
        harnessId,
        sourceIndex,
        sampleAttributes: sampleAttrs,
        sampleValue: Number.isFinite(Number(sampleValue)) ? Number(sampleValue) : undefined,
      });
      setResult(out);
    } catch (err) {
      if (err instanceof TroveIpcError) {
        setError(err.cause.kind === 'internal' ? err.cause.reason : err.cause.kind);
      } else {
        setError(err instanceof Error ? err.message : String(err));
      }
    } finally {
      setBusy(false);
    }
  };

  return (
    <Sheet
      open={open}
      onClose={onClose}
      title="Preview mapping"
      subtitle={
        sourceRule ? (
          <span className="font-mono text-[11px]">
            {harnessId} · rule #{sourceIndex + 1} ({sourceRule.kind})
          </span>
        ) : null
      }
      size="lg"
      footer={
        <div className="flex items-center justify-end gap-2">
          <Button size="sm" variant="ghost" onClick={onClose}>
            Close
          </Button>
          <Button size="sm" variant="primary" onClick={runPreview} loading={busy}>
            Run preview
          </Button>
        </div>
      }
    >
      <div className="grid grid-cols-2 gap-4 text-[12px]">
        <div>
          <h4 className="mb-1 font-medium">Sample input</h4>
          <p className="mb-2 text-[11px] text-fg-tertiary dark:text-fg-tertiary-dark">
            Attributes the raw OTel data point or hook event would carry.
          </p>
          <div className="flex flex-col gap-1">
            {Object.entries(sampleAttrs).map(([k, v]) => (
              <div key={k} className="flex items-center gap-1">
                <code className="flex-1 rounded bg-black/[0.04] px-1.5 py-0.5 font-mono text-[11px]">
                  {k}={v}
                </code>
                <button
                  type="button"
                  className="text-fg-tertiary hover:text-ios-red"
                  onClick={() => {
                    const next = { ...sampleAttrs };
                    delete next[k];
                    setSampleAttrs(next);
                  }}
                  aria-label={`Remove ${k}`}
                >
                  ×
                </button>
              </div>
            ))}
            <div className="flex items-center gap-1">
              <input
                className="flex-1 rounded-md border border-black/[0.08] bg-white px-1.5 py-0.5 font-mono text-[11px] outline-none focus:border-brand focus:ring-1 focus:ring-brand dark:border-white/[0.12] dark:bg-canvas-dark"
                placeholder="key"
                value={pendingKey}
                onChange={(e) => setPendingKey(e.target.value)}
              />
              <span className="text-fg-tertiary">=</span>
              <input
                className="flex-1 rounded-md border border-black/[0.08] bg-white px-1.5 py-0.5 font-mono text-[11px] outline-none focus:border-brand focus:ring-1 focus:ring-brand dark:border-white/[0.12] dark:bg-canvas-dark"
                placeholder="value"
                value={pendingValue}
                onChange={(e) => setPendingValue(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && pendingKey.trim() && pendingValue.trim()) {
                    setSampleAttrs({ ...sampleAttrs, [pendingKey.trim()]: pendingValue.trim() });
                    setPendingKey('');
                    setPendingValue('');
                  }
                }}
              />
              <Button
                size="sm"
                variant="ghost"
                onClick={() => {
                  if (pendingKey.trim() && pendingValue.trim()) {
                    setSampleAttrs({
                      ...sampleAttrs,
                      [pendingKey.trim()]: pendingValue.trim(),
                    });
                    setPendingKey('');
                    setPendingValue('');
                  }
                }}
              >
                add
              </Button>
            </div>
          </div>
          {sourceRule?.kind === 'synthesize-from-native' ? (
            <label className="mt-2 flex items-center gap-2 text-[12px]">
              <span className="w-24 text-fg-tertiary dark:text-fg-tertiary-dark">Value</span>
              <input
                className="flex-1 rounded-md border border-black/[0.08] bg-white px-2 py-1 font-mono text-[12px] outline-none focus:border-brand focus:ring-1 focus:ring-brand dark:border-white/[0.12] dark:bg-canvas-dark"
                value={sampleValue}
                onChange={(e) => setSampleValue(e.target.value)}
              />
            </label>
          ) : null}
        </div>

        <div>
          <h4 className="mb-1 font-medium">Transformed output</h4>
          {error ? <p className="text-[11px] text-ios-red">{error}</p> : null}
          {!error && !result ? (
            <p className="text-[11px] text-fg-tertiary dark:text-fg-tertiary-dark">
              Click <b>Run preview</b> to see what this rule would emit.
            </p>
          ) : null}
          {result ? (
            <>
              {result.emitted ? (
                <div className="flex flex-col gap-1">
                  <p className="font-mono text-[11px]">
                    <code className="rounded bg-ios-green/[0.14] px-1 py-0.5 text-ios-green">
                      {result.emitted.metricName}
                    </code>
                    <span className="ml-2 text-fg-tertiary dark:text-fg-tertiary-dark">
                      ({result.emitted.kind})
                    </span>
                  </p>
                  <p className="font-mono text-[11px]">value: {result.emitted.value}</p>
                  <ul className="flex flex-col gap-0.5">
                    {Object.entries(result.emitted.attributes).map(([k, v]) => (
                      <li
                        key={k}
                        className="font-mono text-[11px] text-fg-secondary dark:text-fg-secondary-dark"
                      >
                        {k} = {v}
                      </li>
                    ))}
                  </ul>
                </div>
              ) : (
                <p className="italic text-[11px] text-fg-tertiary dark:text-fg-tertiary-dark">
                  No emission (rule is suppressed).
                </p>
              )}
              {result.notes.length > 0 ? (
                <ul className="mt-2 flex flex-col gap-0.5">
                  {result.notes.map((n, i) => (
                    <li key={i} className="text-[11px] text-amber-600">
                      ⚠ {n}
                    </li>
                  ))}
                </ul>
              ) : null}
            </>
          ) : null}
        </div>
      </div>
    </Sheet>
  );
}

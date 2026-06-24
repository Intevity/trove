import type { BackendDraft } from '@trove/shared';
import { BOTTOM_PINNED_KINDS, PRESETS, type PresetMetadata } from '@trove/collector-presets';
import { useMemo } from 'react';

import { BackendLogo } from '../../lib/logos.js';
import { Pill } from '../ui/index.js';

export type PresetKind = BackendDraft['kind'];

export interface PresetPickerProps {
  onSelect: (kind: PresetKind) => void;
}

/** Sort order: recommended platform first, then everything else
 *  alphabetically by label, with the generic escape-hatch kinds
 *  pinned at the very bottom. */
function comparePresets(a: PresetMetadata, b: PresetMetadata): number {
  if (a.recommended !== b.recommended) return a.recommended ? -1 : 1;
  const aBottom = BOTTOM_PINNED_KINDS.has(a.kind);
  const bBottom = BOTTOM_PINNED_KINDS.has(b.kind);
  if (aBottom !== bBottom) return aBottom ? 1 : -1;
  return a.label.localeCompare(b.label);
}

export function PresetPicker({ onSelect }: PresetPickerProps): JSX.Element {
  const sortedPresets = useMemo(() => [...PRESETS].sort(comparePresets), []);
  return (
    <section data-testid="preset-picker">
      <h2 className="text-[20px] font-semibold tracking-tight text-fg-primary dark:text-fg-primary-dark">
        Pick a destination for your telemetry
      </h2>
      <p className="mt-1 text-[13px] text-fg-secondary dark:text-fg-secondary-dark">
        Trove forwards every harness&rsquo;s OTLP traffic through a local collector to whichever
        backend you choose. You can swap this later from the dashboard.
      </p>
      <ul className="mt-6 grid gap-3" role="list">
        {sortedPresets.map((preset) => (
          <li key={preset.kind}>
            <button
              type="button"
              onClick={() => onSelect(preset.kind)}
              data-testid={`preset-${preset.kind}`}
              className="group flex w-full items-start gap-3 rounded-tile border border-hairline bg-surface-elevated px-4 py-3 text-left transition hover:bg-canvas hover:shadow-card focus:outline-none focus-visible:ring-2 focus-visible:ring-brand dark:border-hairline-dark dark:bg-surface-elevated-dark dark:hover:bg-canvas-dark"
            >
              <BackendLogo kind={preset.kind} size={32} />
              <span className="flex min-w-0 flex-1 flex-col gap-1">
                <span className="flex w-full items-center justify-between gap-2">
                  <span className="text-[14px] font-medium text-fg-primary dark:text-fg-primary-dark">
                    {preset.label}
                  </span>
                  {preset.recommended ? (
                    <Pill tone="brand" size="xs" testid="preset-recommended-badge">
                      Recommended
                    </Pill>
                  ) : null}
                  {preset.beta ? (
                    <Pill tone="beta" size="xs" testid="preset-beta-badge">
                      Beta
                    </Pill>
                  ) : null}
                </span>
                <span className="text-[12px] text-fg-secondary dark:text-fg-secondary-dark">
                  {preset.description}
                </span>
              </span>
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}

import type { BackendDraft } from '@trove/shared';
import { PRESETS } from '@trove/collector-presets';

import { Pill } from '../ui/index.js';

export type PresetKind = BackendDraft['kind'];

export interface PresetPickerProps {
  onSelect: (kind: PresetKind) => void;
}

export function PresetPicker({ onSelect }: PresetPickerProps): JSX.Element {
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
        {PRESETS.map((preset) => (
          <li key={preset.kind}>
            <button
              type="button"
              onClick={() => onSelect(preset.kind)}
              data-testid={`preset-${preset.kind}`}
              className="group flex w-full flex-col items-start gap-1 rounded-tile border border-hairline bg-surface-elevated px-4 py-3 text-left transition hover:bg-canvas hover:shadow-card focus:outline-none focus-visible:ring-2 focus-visible:ring-ios-blue dark:border-hairline-dark dark:bg-surface-elevated-dark dark:hover:bg-canvas-dark"
            >
              <span className="flex w-full items-center justify-between">
                <span className="text-[14px] font-medium text-fg-primary dark:text-fg-primary-dark">
                  {preset.label}
                </span>
                {preset.recommended ? (
                  <Pill tone="blue" size="xs" testid="preset-recommended-badge">
                    Recommended
                  </Pill>
                ) : null}
              </span>
              <span className="text-[12px] text-fg-secondary dark:text-fg-secondary-dark">
                {preset.description}
              </span>
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}

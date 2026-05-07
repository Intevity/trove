import type { BackendDraft } from '@trove/shared';
import { PRESETS } from '@trove/collector-presets';

export type PresetKind = BackendDraft['kind'];

export interface PresetPickerProps {
  /** Called when the user clicks one of the preset cards. */
  onSelect: (kind: PresetKind) => void;
}

export function PresetPicker({ onSelect }: PresetPickerProps): JSX.Element {
  return (
    <section data-testid="preset-picker">
      <h2 className="text-xl font-semibold text-slate-900 dark:text-slate-100">
        Pick a destination for your telemetry
      </h2>
      <p className="mt-1 text-sm text-slate-600 dark:text-slate-400">
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
              className="flex w-full flex-col items-start gap-1 rounded-lg border border-slate-300 bg-white px-4 py-3 text-left shadow-sm transition hover:border-blue-500 hover:shadow focus:outline-none focus:ring-2 focus:ring-blue-500 dark:border-slate-700 dark:bg-slate-900 dark:hover:border-blue-400"
            >
              <span className="flex w-full items-center justify-between">
                <span className="font-medium text-slate-900 dark:text-slate-100">
                  {preset.label}
                </span>
                {preset.recommended ? (
                  <span
                    className="rounded-full bg-blue-100 px-2 py-0.5 text-xs font-medium text-blue-900 dark:bg-blue-900 dark:text-blue-100"
                    data-testid="preset-recommended-badge"
                  >
                    Recommended
                  </span>
                ) : null}
              </span>
              <span className="text-sm text-slate-600 dark:text-slate-400">
                {preset.description}
              </span>
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}

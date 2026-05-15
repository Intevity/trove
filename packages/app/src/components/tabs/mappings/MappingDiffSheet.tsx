import { createPatch } from 'diff';
import { type JSX } from 'react';

import { type MappingState } from '@trove/shared';

import { Button, Sheet } from '../../ui/index.js';
import { canonicalizeMappingState } from './validate.js';

interface Props {
  open: boolean;
  onClose: () => void;
  persisted: MappingState;
  draft: MappingState;
}

/** Unified-diff view of persisted vs. draft MappingState. Read-only:
 *  the user reviews the diff, then closes the sheet and clicks Apply
 *  on the per-harness card. Uses the canonicalize-then-stringify helper
 *  so the diff is stable across key ordering. */
export function MappingDiffSheet({ open, onClose, persisted, draft }: Props): JSX.Element {
  // Canonicalize for stable diffing (sorted keys, etc.), but render as
  // pretty-printed JSON so the user can read it.
  const a = stableJson(persisted);
  const b = stableJson(draft);
  const patch = createPatch('mappings.json', a, b, '', '', { context: 3 });

  return (
    <Sheet
      open={open}
      onClose={onClose}
      title="Mapping diff"
      subtitle="Changes between persisted state and your draft"
      size="lg"
      footer={
        <Button size="sm" variant="ghost" onClick={onClose}>
          Close
        </Button>
      }
    >
      <pre className="max-h-[60vh] overflow-auto rounded-md bg-canvas p-3 font-mono text-[11px] leading-snug dark:bg-canvas-dark">
        {patch.split('\n').map((line, i) => (
          <span
            key={i}
            className={
              line.startsWith('+') && !line.startsWith('+++')
                ? 'block text-ios-green'
                : line.startsWith('-') && !line.startsWith('---')
                  ? 'block text-ios-red'
                  : line.startsWith('@@')
                    ? 'block text-brand'
                    : 'block text-fg-secondary dark:text-fg-secondary-dark'
            }
          >
            {line || ' '}
          </span>
        ))}
      </pre>
    </Sheet>
  );
}

/** Pretty-print the state with sorted keys so the diff is stable
 *  regardless of how the underlying records were constructed. */
function stableJson(state: MappingState): string {
  // Canonicalize produces a sorted single-line string; round-trip
  // through JSON.parse + JSON.stringify(indent=2) to get a readable
  // pretty-printed form that's still in sorted key order.
  return JSON.stringify(JSON.parse(canonicalizeMappingState(state)), null, 2);
}

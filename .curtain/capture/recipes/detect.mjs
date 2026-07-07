/*
 * detect — one launch sweeps every standard install path. Land on Harnesses, hit
 * Refresh to reveal a rich detected set spanning all three coverage badges
 * (Auto-detected / Partial Coverage / Best Effort), pause on the badge variety.
 * DEMO_SCRIPTS §detect.
 */
import { detectedRow } from './_shared.mjs';

export const recipe = {
  run: async (c) => {
    await c.tapTestId('tab-harnesses');
    await c.sleep(700);
    // A detected set with one of each badge tone (badges are keyed by harness id).
    await c.seed({
      detectedHarnesses: [
        detectedRow('claude-code', 'on', 'config-dir'),
        detectedRow('codex-cli', 'on', 'config-dir'),
        detectedRow('cursor-ide', 'on', 'config-dir'),
        detectedRow('cursor-cli', 'unknown', 'config-dir'),
        detectedRow('aider', 'unknown', 'path-binary'),
        detectedRow('claude-desktop', 'on', 'app-bundle', { adapterAvailable: false }),
        detectedRow('qwen-code', 'off', 'config-dir'),
      ],
    });
    await c.tapText('Refresh');
    await c.sleep(1100);
    await c.hoverTestId('harness-badge-cursor-cli');
    await c.sleep(800);
    await c.hoverTestId('harness-badge-aider');
    await c.sleep(800);
    const f = await c.focalOfTestId('harness-list', 0, 180);
    c.zoom({ startMs: 2600, endMs: c.now(), cx: f.cx, cy: f.cy, zmax: 1.2 });
    await c.sleep(1200);
  },
};

/*
 * enable — toggle a row and Trove patches that tool to emit OTLP. Click Enable on
 * the disabled qwen-code row, linger on the managed patch before/after, then Apply;
 * the row flips green. DEMO_SCRIPTS §enable.
 */
import { baseDetected, baseEnabled } from './_shared.mjs';

export const recipe = {
  run: async (c) => {
    await c.tapTestId('tab-harnesses');
    await c.sleep(700);
    await c.hoverTestId('harness-row-qwen-code');
    await c.sleep(600);
    await c.tapRole('button', 'toggle-qwen-code'); // opens the patch preview
    await c.waitText('Apply Trove patch');
    const z0 = c.now();
    await c.sleep(1600); // hold on the sentinel-bracketed managed block
    const fm = await c.focalOfTestId('patch-preview-modal', 0, 200);
    // Pre-seed the enabled end-state so the post-apply refresh flips the row green.
    await c.seed({
      detectedHarnesses: baseDetected({ qwen: 'on' }),
      appState: { harnesses: baseEnabled(['qwen-code']) },
    });
    await c.tapTestId('patch-preview-apply');
    await c.sleep(1500);
    c.zoom({ startMs: z0, endMs: z0 + 1600, cx: fm.cx, cy: fm.cy, zmax: 1.2 });
  },
};

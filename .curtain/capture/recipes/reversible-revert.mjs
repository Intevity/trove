/*
 * reversible-revert — every change reverts byte-for-byte. Enable qwen-code (show the
 * managed patch), Apply → row green, then Disable → the row returns to its original
 * disabled state, byte-for-byte. DEMO_SCRIPTS §reversible-revert.
 */
import { baseDetected, baseEnabled } from './_shared.mjs';

export const recipe = {
  run: async (c) => {
    await c.tapTestId('tab-harnesses');
    await c.sleep(700);
    await c.tapRole('button', 'toggle-qwen-code'); // enable → patch preview
    await c.waitText('Apply Trove patch');
    const z0 = c.now();
    await c.sleep(1500); // linger on the sentinel-bracketed managed block
    const fm = await c.focalOfTestId('patch-preview-modal', 0, 200);
    await c.seed({
      detectedHarnesses: baseDetected({ qwen: 'on' }),
      appState: { harnesses: baseEnabled(['qwen-code']) },
    });
    await c.tapTestId('patch-preview-apply');
    await c.sleep(1300); // row green
    // Revert: disable is a direct revert_patch (no modal); row returns to original.
    await c.seed({
      detectedHarnesses: baseDetected({ qwen: 'off' }),
      appState: { harnesses: baseEnabled() },
    });
    await c.tapRole('button', 'toggle-qwen-code');
    await c.sleep(1500);
    c.zoom({ startMs: z0, endMs: z0 + 1500, cx: fm.cx, cy: fm.cy, zmax: 1.2 });
  },
};

/*
 * native-otel — flip the flag, route the stream. Show the native harnesses already
 * emitting OTel ("Telemetry on"), then enable another native harness (qwen-code) so
 * its stream routes straight through — no adapter. DEMO_SCRIPTS §native-otel.
 */
import { baseDetected, baseEnabled } from './_shared.mjs';

export const recipe = {
  run: async (c) => {
    await c.tapTestId('tab-harnesses');
    await c.sleep(700);
    await c.hoverTestId('harness-telemetry-claude-code');
    await c.sleep(700);
    await c.hoverTestId('harness-telemetry-codex-cli');
    await c.sleep(700);
    await c.tapRole('button', 'toggle-qwen-code');
    await c.waitText('Apply Trove patch');
    await c.sleep(1000);
    await c.seed({
      detectedHarnesses: baseDetected({ qwen: 'on' }),
      appState: { harnesses: baseEnabled(['qwen-code']) },
    });
    await c.tapTestId('patch-preview-apply');
    await c.sleep(1400);
    const f = await c.focalOfTestId('harness-row-qwen-code');
    c.zoom({ startMs: 3400, endMs: c.now(), cx: f.cx, cy: f.cy, zmax: 1.3 });
    await c.sleep(900);
  },
};

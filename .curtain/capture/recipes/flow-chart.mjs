/*
 * flow-chart — live data-flow chart. Enable a 4th harness (crossing the cluster
 * threshold) so the harnesses collapse into the Orbital Hub, then fire Test Pipeline
 * and watch signal dots pulse hub → collector → backends. DEMO_SCRIPTS §flow-chart.
 */
import { baseDetected, baseEnabled, risingSnapshot } from './_shared.mjs';

export const recipe = {
  run: async (c) => {
    await c.tapTestId('tab-harnesses');
    await c.sleep(600);
    await c.tapRole('button', 'toggle-qwen-code');
    await c.waitText('Apply Trove patch');
    await c.sleep(900);
    // 4 enabled harnesses (> CLUSTER_THRESHOLD) → the Orbital Hub cluster renders.
    await c.seed({
      detectedHarnesses: baseDetected({ qwen: 'on' }),
      appState: { harnesses: baseEnabled(['qwen-code']) },
    });
    await c.tapTestId('patch-preview-apply');
    await c.sleep(900);
    await c.tapTestId('tab-overview');
    await c.sleep(700);
    await c.scrollTo(320);
    await c.sleep(800);
    await c.tapText('Test Pipeline');
    const startMs = c.now();
    for (let i = 1; i <= 4; i++) {
      await c.emit('metrics-snapshot', risingSnapshot(i));
      await c.sleep(600);
    }
    const f = await c.focalOfTestId('flow-chart-cluster-harness', 60, 40);
    c.zoom({ startMs, endMs: c.now(), cx: f.cx, cy: f.cy, zmax: 1.4 });
    await c.sleep(1000);
  },
};

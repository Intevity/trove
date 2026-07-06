/*
 * health — a 4-color pill per backend, driven by the collector itself. Settle on
 * the Overview Diagnostics panel, run a backend check, hold on the all-green rows.
 * DEMO_SCRIPTS §health.
 */
import { risingSnapshot } from './_shared.mjs';

export const recipe = {
  run: async (c) => {
    await c.tapTestId('tab-overview');
    await c.sleep(700);
    await c.hoverTestId('diagnostics-row-sidecar');
    await c.sleep(500);
    await c.hoverTestId('diagnostics-row-backend');
    await c.sleep(500);
    await c.tapTestId('diagnostics-backend-check-button');
    await c.emit('metrics-snapshot', risingSnapshot(1));
    await c.sleep(1300);
    const f = await c.focalOfTestId('diagnostics-panel', 0, 140);
    c.zoom({ startMs: 1900, endMs: c.now(), cx: f.cx, cy: f.cy, zmax: 1.3 });
    await c.sleep(1000);
  },
};

/*
 * fan-out — every signal broadcasts to every backend you enable. Show the Platforms
 * list (both backends enabled), cut to the Overview flow, fire Test Pipeline and
 * watch signals fan out to both backend nodes. DEMO_SCRIPTS §fan-out.
 */
import { risingSnapshot } from './_shared.mjs';

export const recipe = {
  run: async (c) => {
    await c.tapTestId('tab-platforms');
    await c.sleep(900);
    await c.hoverTestId('platforms-list');
    await c.sleep(900);
    await c.tapTestId('tab-overview');
    await c.sleep(700);
    await c.scrollTo(320);
    await c.sleep(700);
    await c.tapText('Test Pipeline');
    const startMs = c.now();
    for (let i = 1; i <= 4; i++) {
      await c.emit('metrics-snapshot', risingSnapshot(i));
      await c.sleep(600);
    }
    const f = await c.focalOfTestId('flow-chart-svg', 0, 240);
    c.zoom({ startMs, endMs: c.now(), cx: f.cx, cy: f.cy, zmax: 1.25 });
    await c.sleep(1000);
  },
};

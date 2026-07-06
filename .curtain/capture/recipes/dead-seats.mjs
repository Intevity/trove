/*
 * dead-seats — find paid seats with zero activity. On Harnesses, contrast an active
 * "Telemetry on" row against an enabled-but-silent "Telemetry off" row. (Trove has
 * no dedicated dead-seat panel; the story is carried by the telemetry-pill contrast.)
 * DEMO_SCRIPTS §dead-seats.
 */
export const recipe = {
  run: async (c) => {
    await c.tapTestId('tab-harnesses');
    await c.sleep(800);
    await c.hoverTestId('harness-telemetry-claude-code'); // active seat
    await c.sleep(1000);
    await c.hoverTestId('harness-telemetry-qwen-code'); // silent / dead seat
    await c.sleep(1000);
    const f = await c.focalOfTestId('harness-row-qwen-code');
    c.zoom({ startMs: 2000, endMs: c.now(), cx: f.cx, cy: f.cy, zmax: 1.3 });
    await c.sleep(1200);
  },
};

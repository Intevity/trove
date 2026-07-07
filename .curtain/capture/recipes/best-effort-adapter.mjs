/*
 * best-effort-adapter — watchers derive OTLP from logs. Show aider's "Best Effort"
 * badge on Harnesses, then cut to the Mappings tab and settle on aider's Hook rules
 * (when → emit Tier A) that translate raw events into Tier A. DEMO_SCRIPTS §best-effort-adapter.
 */
export const recipe = {
  run: async (c) => {
    await c.tapTestId('tab-harnesses');
    await c.sleep(700);
    await c.hoverTestId('harness-badge-aider'); // "Best Effort"
    await c.sleep(1100);
    await c.tapTestId('tab-mappings');
    await c.sleep(900);
    await c.waitText('Hook rules');
    await c.scrollTo(920); // down to the aider card's hook rules
    await c.sleep(1100);
    c.zoom({ startMs: 2800, endMs: c.now(), cx: 960, cy: 560, zmax: 1.25 });
    await c.sleep(1300);
  },
};

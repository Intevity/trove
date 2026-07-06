/*
 * cost-normalization — Tier A makes vendors comparable. On the Mappings tab, show
 * two harness cards (claude-code, codex-cli) whose native cost signals both
 * synthesize into the SAME trove.harness.cost.usd target. DEMO_SCRIPTS §cost-normalization.
 */
export const recipe = {
  run: async (c) => {
    await c.tapTestId('tab-mappings');
    await c.sleep(900);
    await c.scrollTo(380);
    await c.sleep(900);
    await c.waitText('claude-code');
    await c.sleep(800);
    await c.scrollTo(820); // down to the codex-cli card sharing the cost.usd target
    await c.sleep(1100);
    c.zoom({ startMs: 2700, endMs: c.now(), cx: 960, cy: 520, zmax: 1.2 });
    await c.sleep(1300);
  },
};

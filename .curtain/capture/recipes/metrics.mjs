/*
 * metrics (Tier A) — a normalized cross-harness schema for true side-by-side cost.
 * Land on the Mappings tab and settle on the Metrics catalog: the five builtin
 * trove.harness.* Tier A metrics every harness maps into. Catalog + rules come from
 * the shared init.js seed. DEMO_SCRIPTS §metrics.
 */
export const recipe = {
  run: async (c) => {
    await c.tapTestId('tab-mappings');
    await c.sleep(1000);
    await c.waitText('Metrics catalog');
    await c.hoverTestId('catalog-add-toggle');
    await c.sleep(700);
    await c.scrollTo(120);
    await c.sleep(900);
    const f = await c.focalOfTestId('catalog-add-toggle', 0, 80);
    c.zoom({ startMs: 1600, endMs: c.now(), cx: f.cx, cy: f.cy, zmax: 1.3 });
    await c.sleep(1500);
  },
};

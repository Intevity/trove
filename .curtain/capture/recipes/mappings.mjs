/*
 * mappings — a visual editor from native signal to Tier A. On the Mappings tab,
 * settle on the claude-code card's Synthesis rules, flip Visual → JSON → Visual to
 * show both representations of the same mapping. DEMO_SCRIPTS §mappings.
 */
export const recipe = {
  run: async (c) => {
    await c.tapTestId('tab-mappings');
    await c.sleep(900);
    await c.scrollTo(360);
    await c.sleep(900);
    await c.waitText('Synthesis rules');
    await c.sleep(700);
    await c.tapText('JSON'); // the same mapping as raw config
    await c.sleep(1500);
    await c.tapText('Visual'); // back to the visual editor
    await c.sleep(900);
    c.zoom({ startMs: 3400, endMs: c.now(), cx: 960, cy: 560, zmax: 1.2 });
    await c.sleep(1200);
  },
};

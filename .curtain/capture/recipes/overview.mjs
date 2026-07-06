/*
 * Overview — "one pane of glass". Settle on the green Diagnostics, sweep down to
 * the Data flow chart, fire Test Pipeline, and let the Collector counters climb
 * live while signal dots stream through the flow; push in on the flow at the end.
 *
 * A recipe is `{ run: async (ctx) => {...} }`. `ctx` is @curtain/capture's recipe
 * context (moveTo/tapText/seed/emit/zoom/…); the mock + window hooks come from
 * ../init.js. See documentation/DEMO_SCRIPTS.md §"overview".
 */

/** A rising metrics snapshot — received === sent so the pipeline reads "healthy". */
function snap(n) {
  const received = { spans: 128 + n * 12, metricPoints: 64 + n * 6, logRecords: 32 + n * 3 };
  return {
    received,
    sent: received,
    lastSignalMsAgo: 400,
    scrapedMsAgo: 300,
    unreachable: false,
    overallHealth: 'green',
    diagObservations: {
      'claude-code': { spans: 60 + n * 6, metricPoints: 30 + n * 3, logRecords: 12 + n },
    },
  };
}

export const recipe = {
  run: async (c) => {
    await c.sleep(900);
    await c.moveTo(960, 360, 700); // drift over Diagnostics
    await c.sleep(700);
    await c.moveTo(960, 620, 800); // down to the Data flow chart
    await c.sleep(800);
    await c.tapText('Test Pipeline'); // kick a synthetic signal burst
    const startMs = c.now();
    for (let i = 1; i <= 4; i++) {
      await c.emit('metrics-snapshot', snap(i));
      await c.sleep(650);
    }
    c.zoom({ startMs, endMs: c.now(), cx: 960, cy: 620, zmax: 1.35 });
    await c.moveTo(960, 620, 500);
    await c.sleep(1200);
  },
};

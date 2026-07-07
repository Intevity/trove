/*
 * localhost-only — binds 127.0.0.1, forwards only to you. Show the Platforms list
 * (localhost endpoints), cut to Logs, and stream collector lines all exporting to
 * 127.0.0.1 — nothing leaves the machine except to your own backends.
 * DEMO_SCRIPTS §localhost-only.
 */
export const recipe = {
  run: async (c) => {
    await c.tapTestId('tab-platforms');
    await c.sleep(800);
    await c.hoverTestId('platforms-list');
    await c.sleep(900);
    await c.tapTestId('tab-logs');
    await c.sleep(800);
    await c.waitText('127.0.0.1');
    for (let i = 0; i < 5; i++) {
      await c.emit('collector-log', {
        stream: 'stdout',
        line: 'exporting ' + (120 + i * 12) + ' spans -> 127.0.0.1:14318',
      });
      await c.sleep(600);
    }
    const f = await c.focalOfTestId('logs-output', 0, 120);
    c.zoom({ startMs: 2600, endMs: c.now(), cx: f.cx, cy: f.cy, zmax: 1.25 });
    await c.sleep(1000);
  },
};

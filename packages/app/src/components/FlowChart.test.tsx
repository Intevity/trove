import { describe, expect, it } from 'vitest';

import { resolveHarnessRate } from './FlowChart.js';

const AGGREGATE = { spans: 0, metrics: 5, logs: 1 };
const IDLE = { spans: 0, metrics: 0, logs: 0 };

describe('resolveHarnessRate', () => {
  it('uses a harness own diag lane when present', () => {
    const per = { 'codex-cli': { spans: 0, metrics: 3, logs: 0 } };
    expect(resolveHarnessRate('codex-cli', per, AGGREGATE)).toEqual({
      spans: 0,
      metrics: 3,
      logs: 0,
    });
  });

  it('does NOT light up an idle Cursor IDE just because something else is flowing', () => {
    // cursor-cli lane is present and idle (no cursor traffic); the
    // aggregate is non-zero because e.g. a Claude Code session is active.
    const per = { 'cursor-cli': IDLE, 'claude-code': { spans: 0, metrics: 5, logs: 1 } };
    // Pre-fix this returned AGGREGATE (cursor-ide absent → fell through).
    expect(resolveHarnessRate('cursor-ide', per, AGGREGATE)).toEqual(IDLE);
  });

  it('reflects real Cursor activity via the shared cursor-cli lane', () => {
    const per = { 'cursor-cli': { spans: 0, metrics: 2, logs: 4 } };
    expect(resolveHarnessRate('cursor-ide', per, AGGREGATE)).toEqual({
      spans: 0,
      metrics: 2,
      logs: 4,
    });
  });

  it('aliases Codex Desktop onto the Codex CLI lane', () => {
    const per = { 'codex-cli': IDLE };
    expect(resolveHarnessRate('codex-desktop', per, AGGREGATE)).toEqual(IDLE);
  });

  it('treats an aliased harness as idle when the sibling lane is absent', () => {
    expect(resolveHarnessRate('cursor-ide', {}, AGGREGATE)).toEqual(IDLE);
  });

  it('still falls back to the aggregate for genuine no-attribution emitters', () => {
    // Cline / Sentinel have no diag lane and no alias — keep the previous
    // "reflect something is flowing" behaviour.
    expect(resolveHarnessRate('cline', {}, AGGREGATE)).toEqual(AGGREGATE);
    expect(resolveHarnessRate('sentinel', {}, AGGREGATE)).toEqual(AGGREGATE);
  });
});

import { describe, expect, it } from 'vitest';

import {
  SYNTHETIC_SERVICE_NAME,
  SYNTHETIC_SPAN_ID,
  SYNTHETIC_SPAN_NAME,
  SYNTHETIC_TRACE_ID,
} from './constants.js';

/** Locked-in canary values. Drift here means the user sees one trace ID
 *  in the dashboard hints and a different one lands at their backend.
 *  Rust side asserts the same values in `test_export.rs` —
 *  `synthetic_payload_carries_recognisable_canary_ids`. */
describe('synthetic canary constants', () => {
  it('matches the Rust source-of-truth canaries', () => {
    expect(SYNTHETIC_SERVICE_NAME).toBe('trove-test-export');
    expect(SYNTHETIC_SPAN_NAME).toBe('test_export');
    expect(SYNTHETIC_TRACE_ID).toBe('74726f7665740000c0deca5e0d5705ed');
    expect(SYNTHETIC_SPAN_ID).toBe('74726f7665747465');
  });

  it('trace id is 32 lowercase hex chars', () => {
    expect(SYNTHETIC_TRACE_ID).toMatch(/^[0-9a-f]{32}$/);
  });

  it('span id is 16 lowercase hex chars', () => {
    expect(SYNTHETIC_SPAN_ID).toMatch(/^[0-9a-f]{16}$/);
  });
});

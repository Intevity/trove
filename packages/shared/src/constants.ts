/** Canary identifiers baked into the synthetic OTLP payload sent by
 *  the wizard's "Test export" button and the dashboard's "Test
 *  Pipeline" button. The Rust side in
 *  `packages/app/src-tauri/src/ipc/test_export.rs` holds the
 *  authoritative copies; these TS constants are pinned identically so
 *  the UI can tell the user what to search for in their observability
 *  tool without having to round-trip the values through IPC.
 *
 *  A Vitest in `constants.test.ts` snapshots these values; a Rust
 *  unit test (`synthetic_payload_carries_recognisable_canary_ids`)
 *  asserts the Rust source matches. Drift between the two sides
 *  surfaces as a test failure rather than a silent UI lie. */

export const SYNTHETIC_SERVICE_NAME = 'trove-test-export';
export const SYNTHETIC_SPAN_NAME = 'test_export';
export const SYNTHETIC_TRACE_ID = '74726f7665740000c0deca5e0d5705ed';
export const SYNTHETIC_SPAN_ID = '74726f7665747465';

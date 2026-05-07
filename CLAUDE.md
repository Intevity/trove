<!-- plumb:start -->

## Plumb-managed: assumption declaration protocol

Plumb is installed in this project. Before any code edit (Edit/Write tool use),
declare the assumptions your change relies on as a fenced JSON block tagged
`plumb-assumptions`, e.g.:

```plumb-assumptions
[
  {
    "entity": "function getUser",
    "property": "returns null on missing user",
    "rationale": "matches existing call sites in src/api/user.ts",
    "verification_strategy": { "type": "ast_query", "language": "typescript", "query": "call:getUser return" },
    "confidence": "medium",
    "source": "verified"
  }
]
```

Each entry MUST include a `verification_strategy`. Pick the most specific
variant that fits — Plumb runs the strategy against your codebase and
records confirmed/refuted/inconclusive in the calibration ledger. Defaulting
to `manual` produces no verifier signal, so reach for it only when nothing
else applies:

- `{ "type": "grep_pattern", "pattern": "<ripgrep pattern>" }` — confirm presence/absence in the codebase
- `{ "type": "ast_query", "language": "typescript", "query": "<query>" }` — confirm via AST shape
- `{ "type": "file_read", "filePath": "<path>" }` — confirm by reading a specific file
- `{ "type": "type_check", "snippet": "<typescript snippet>" }` — confirm by compiling a tiny snippet
- `{ "type": "test_run", "pattern": "<vitest pattern>" }` — confirm by running targeted tests
- `{ "type": "manual", "instructions": "<one-line instructions>" }` — fallback only when none of the above can verify it

Set `source` honestly — the calibration aggregate uses it to score how
often each kind of claim holds up:

- `"verified"` — you saw direct evidence in the visible code/files
- `"assumed"` — you're extrapolating from naming/conventions/types
- `"recalled"` — you're restating a stable, well-known library/API fact

The Plumb daemon verifies each assumption against the codebase and surfaces
contradictions through the next tool-use hook.

<!-- plumb:end -->

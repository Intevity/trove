# MAPPING_PLAN

Status: **implemented (Sprint 13) + extended (v2)** — core mapping
foundation, collector overlay, and watcher/wrapper Tier A emission
shipped in Sprint 13. The follow-up editor (per-row edits, drag-reorder,
preview, diff, JSON escape hatch) shipped in v2, alongside a
user-customizable metric catalog.

**v2 supersedes one of the non-goals below.** The original document
listed "Custom metric names" as out-of-scope on the grounds that
inventing new Tier A metrics breaks cross-user dashboards. The v2
editor lifts that constraint via a `MappingState.metrics` catalog that
preserves the five builtins as locked entries and allows additional
custom counters/gauges/histograms. The cross-user-dashboard caveat now
lives as an in-product warning on the "Add custom metric" form (and as
a known limitation: hook/watcher harnesses don't yet honor custom
metric rules — only native-OTel harnesses do, via the collector
codegen). Custom histograms inherit the existing single bucket-bound
set in v2; configurable bounds per metric is a future PR.

## Implementation summary

What's in the codebase now:

- `crate::mappings` (`packages/app/src-tauri/src/mappings/`) — Rust
  data model (`MappingState`, `HarnessMapping`, `MappingSource`,
  `TierAMetric`, `CostOverride`), per-harness defaults, validation,
  and the cost rate table. Wire-format byte-identical to the Zod
  schemas in `@trove/shared`.
- `AppState.mappings` field (schema v6) — persisted alongside the
  existing identity/backend records. v5→v6 migration auto-populates
  per-harness defaults via `mappings::default_state` so users see
  Tier A coverage on first relaunch.
- `apply_mapping_overlay` (`collector/codegen.rs`) — extends the
  rendered `collector.yaml` with `transform/harness-tag` (back-fills
  `harness.id` from `service.name` for native-OTel harnesses that
  can't inject `OTEL_RESOURCE_ATTRIBUTES`) plus one
  `metricstransform/tierA-<harness>` block per harness with enabled
  `synthesize-from-native` rows. Uses `action: insert` so Tier B
  passes through untouched.
- `apply_mappings` / `reset_mappings_to_defaults` IPC commands —
  validate → persist → reload collector.
- React `MappingsTab` (read-only viewer) — surfaces every harness's
  rows with a master enable toggle and a "reset to defaults" button.
- `cline_watcher` Tier A emission — classifies each new `ui_messages.json`
  entry by `say` type and emits `trove.harness.events` / `errors`
  data points alongside the existing logs.
- `wrapper_common_metrics` shared builder — Aider and Copilot-CLI
  wrappers each invocation produces `trove.harness.events` +
  `trove.harness.turn.duration` (histogram, shared bucket bounds with
  the Cursor hook) + `trove.harness.errors` on non-zero exit.

Deferred to a follow-up PR:

- Per-row mapping editor (add/edit/delete individual rows; the v1 UI
  is master-toggle + reset-to-defaults only).
- Per-model cost-override editor.
- Export / import mapping JSON (open question #4).

## Goal

Let users see and customize how Trove maps raw harness signals onto its
Tier A metrics schema. Defaults ship with each harness; the UI exposes
the defaults, the user can edit them, and edits live-apply via a
collector restart.

This document is the design contract. The implementing session should
not re-litigate the schema or defaults below — those are settled. The
work is the data model, UI, persistence, and collector wiring.

## Non-goals

- Editing Tier B (harness-native) metrics. Those flow through
  untouched; Trove neither renames nor reshapes them.
- A query/transform language. The user configures _which signals map
  to which Tier A metric attributes_, not arbitrary expressions.
- ~~Custom metric names. Tier A is a fixed five-metric schema (see
  §"Tier A schema" below). Users cannot rename `trove.harness.events`
  or invent new Tier A metrics — that breaks cross-user dashboards.~~
  **(Superseded in v2.)** Users may now extend the catalog with custom
  metric definitions. The five builtins remain locked (rename and
  delete disabled) so cross-user dashboards keyed on the original
  Tier A schema continue to interpret data the same way. The custom
  metric form surfaces a warning that custom names won't carry across
  teams unless the consuming side has the same catalog.

## Background: the schema this configures

Decided in the design conversation that preceded this plan (see git
log for the conversation context). Tier A is the cross-harness
metrics schema:

```
trove.harness.events         Sum (Δ, mono)    event.kind ∈ {chat.turn, tool.call,
                                              shell.exec, file.edit,
                                              session.start, session.end}
trove.harness.tokens         Sum (Δ, mono)    direction ∈ {input, output}, model
trove.harness.cost.usd       Sum (Δ, mono)    model, cost.method ∈ {exact, estimated}
trove.harness.turn.duration  Histogram (s)    event.kind
trove.harness.errors         Sum (Δ, mono)    error.kind ∈ {rate_limit, auth,
                                              tool_failure, network, policy,
                                              unknown}
```

Resource attributes set per-harness (not per-point):
`service.name`, `harness.id`, `harness.name`, `user.name`, `user.email`,
`trove.source`.

Sub-classification (tool.name, file.path, shell.exit_code,
conversation.id, etc.) lives on **logs**, not metrics, and is out of
scope for this mapping UI.

## User stories

1. _"I want to see what events Cursor reports as `trove.harness.events`
   and rename one of them."_ — User opens Cursor's mapping in Settings,
   sees `beforeShellExecution → shell.exec`, `afterShellExecution →
shell.exec`, `beforeSubmitPrompt → chat.turn`, `afterAgentResponse →
chat.turn`. They edit the prompt-submit mapping to be `chat.turn`
   only on `afterAgentResponse` (to avoid double-counting).
2. _"I don't trust the estimated cost for Cursor — turn it off."_ —
   User toggles the `cost.usd` row for Cursor to "disabled." The metric
   stops emitting for that harness.
3. _"My company runs a custom LLM at $0.0008/1k input tokens."_ — User
   adds a model entry to the cost rate table with their own per-1k
   prices. Trove uses those rates for the matching `model` attribute.
4. _"Claude Code emits `claude_code.token.usage`. I want it also
   counted in `trove.harness.tokens` so my cross-harness dashboard
   works."_ — User toggles "synthesize from native" on the Claude Code
   tokens mapping. The collector pipeline gains a `transform`
   processor stanza that copies the native metric into Tier A.

## Data model

Mappings persist in `state.json` under a new top-level key. Add a
schema migration; this is a v5 → v6 step.

```ts
// packages/shared/src/schemas.ts
export const TierAMetric = z.enum(['events', 'tokens', 'cost.usd', 'turn.duration', 'errors']);

export const MappingSource = z.discriminatedUnion('kind', [
  // Hook/watcher harnesses: explicit rules per raw event name.
  z.object({
    kind: z.literal('hook-rule'),
    when: z.string(), // raw event name e.g. "beforeShellExecution"
    emit: z
      .object({
        // Which Tier A metric this contributes to.
        metric: TierAMetric,
        // The event.kind / error.kind / direction the row applies to.
        attributes: z.record(z.string(), z.string()),
      })
      .nullable(), // null = "don't emit anything"
  }),
  // Native-OTel harnesses: passthrough is implicit; this row only
  // exists when the user opts in to Tier A synthesis.
  z.object({
    kind: z.literal('synthesize-from-native'),
    nativeMetric: z.string(), // e.g. "claude_code.token.usage"
    targetMetric: TierAMetric,
    attributeMap: z.record(z.string(), z.string()), // raw key → Tier A key
  }),
]);

export const HarnessMapping = z.object({
  harnessId: HarnessId,
  enabled: z.boolean(),
  sources: z.array(MappingSource),
  // Per-harness rate-table overrides for cost.usd. Empty = use Trove
  // defaults. Keyed by model name.
  costOverrides: z
    .record(
      z.string(),
      z.object({
        inputUsdPer1kTokens: z.number(),
        outputUsdPer1kTokens: z.number(),
      }),
    )
    .default({}),
});

export const MappingState = z.object({
  schemaVersion: z.literal(1),
  harnesses: z.array(HarnessMapping),
});
```

In Rust, mirror as `crate::mappings::MappingState` with serde
`#[serde(rename_all = "camelCase")]`. Store alongside `state.json` in
the same atomic-write path.

## Defaults

Each harness ships with a default `HarnessMapping` baked into the Rust
adapter. When a user enables a harness for the first time, Trove
copies the default into `MappingState`. Subsequent edits mutate the
copy; reverts restore the default.

**Default mapping per harness** (the implementing session should
expand these into Rust constants):

| Harness         | Default Tier A rows                                                                                                                                                                                                                                                   | Notes                                                                               |
| --------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| claude-code     | `events`: synthesized from `claude_code.session.count` → `chat.turn`; `tool.decision.count` → `tool.call`. `tokens`: from `claude_code.token.usage`. `cost.usd`: from `claude_code.cost.usage`, method=`exact`.                                                       | All Tier B passes through; synthesis is additive.                                   |
| codex-cli       | Similar to claude-code — codex's native metrics map to chat.turn / tool.call / tokens / cost.                                                                                                                                                                         |                                                                                     |
| antigravity-cli | Hook-bridged (like Cursor): the Trove-shipped hook emits Tier A inline. `events`: agent event → `chat.turn` / `tool.call`. `cost.usd`: estimated from response bytes `/ 4 * rate(model)`, method=`estimated`.                                                         |                                                                                     |
| qwen-code       | Native `telemetry.*` (`gemini_cli.*`) → Tier A via collector `transform`. Still ships the legacy Gemini CLI fork mechanism.                                                                                                                                           |                                                                                     |
| cursor (IDE)    | `events`: `beforeSubmitPrompt` → null (avoid double-count); `afterAgentResponse` → `chat.turn`; `beforeShellExecution` → null; `afterShellExecution` → `shell.exec`. `cost.usd`: estimated from `cursor.response.bytes / 4 * rate(cursor.model)`, method=`estimated`. | Cursor's hook may omit `cursor.model` for some events — those rows can't emit cost. |
| cursor-cli      | Same as cursor IDE — shared hook script, shared mapping.                                                                                                                                                                                                              | The umbrella `cursor` harness id covers both.                                       |
| cline           | `events`: tail `ui_messages.json`; classify by message type → chat.turn / tool.call / file.edit. `tokens`: from per-task token counts. `cost.usd`: exact from Cline's token×rate.                                                                                     |                                                                                     |
| aider           | `events`: one `chat.turn` per wrapper invocation. `tokens`: estimated from prompt/response bytes (no API interception). `cost.usd`: estimated, method=`estimated`.                                                                                                    |                                                                                     |
| copilot-cli     | Same as aider.                                                                                                                                                                                                                                                        |                                                                                     |
| opencode        | TBD when adapter lands.                                                                                                                                                                                                                                               |                                                                                     |

Cost rate table default (`packages/app/src-tauri/src/mappings/rates.rs`):

```rust
pub const DEFAULT_RATES: &[(&str, &str, f64, f64)] = &[
    // (provider, model, input_usd_per_1k_tokens, output_usd_per_1k_tokens)
    ("anthropic", "claude-opus-4",        15.0, 75.0),
    ("anthropic", "claude-sonnet-4",       3.0, 15.0),
    ("anthropic", "claude-haiku-4",        1.0,  5.0),
    ("openai",    "gpt-4o",                2.5, 10.0),
    ("openai",    "gpt-4o-mini",           0.15, 0.6),
    ("openai",    "o1",                   15.0, 60.0),
    ("google",    "gemini-2.0-flash",      0.075, 0.3),
    ("google",    "gemini-2.5-pro",        1.25, 10.0),
    // ... expand as needed. Conservative defaults to revisit at
    // each Trove release; users override per-model in their state.
];
```

## UI design

New tab in Settings (sibling to existing Backend / Identity panes):
**Settings → Metric Mapping**.

```
┌─ Metric Mapping ─────────────────────────────────────────────────┐
│                                                                  │
│ How Trove turns raw harness activity into the cross-harness      │
│ metrics shown on your dashboard. Click a harness to see and      │
│ edit its mapping. Tier B (harness-native) metrics always pass    │
│ through unchanged — these settings only affect Tier A.           │
│                                                                  │
│ ▼ cursor                                                Enabled ☑│
│   ┌────────────────────────────────────────────────────────────┐ │
│   │ Source event           Maps to                              │ │
│   │ ─────────────────────  ────────────────────────────────     │ │
│   │ beforeShellExecution   ⊘ none           [edit] [reset]      │ │
│   │ afterShellExecution    events: shell.exec                   │ │
│   │ beforeSubmitPrompt     ⊘ none                               │ │
│   │ afterAgentResponse     events: chat.turn                    │ │
│   │                                                              │ │
│   │ Cost estimation        cost.usd (estimated from bytes)      │ │
│   │   Per-1k rates: defaults  [override...]                     │ │
│   └────────────────────────────────────────────────────────────┘ │
│                                                                  │
│ ▶ claude-code                                          Enabled ☑│
│ ▶ cline                                                Enabled ☑│
│ ▶ codex-cli                                            Enabled ☐│
│ ...                                                              │
│                                                                  │
│ [Apply changes]  [Reset all to defaults]                        │
└──────────────────────────────────────────────────────────────────┘
```

Edit-row dialog (per source event):

```
┌─ Edit mapping: cursor / afterAgentResponse ──────────────────────┐
│                                                                  │
│ Emit Tier A metric:  [● events] [○ tokens] [○ errors] [○ none]  │
│                                                                  │
│ Attributes:                                                      │
│   event.kind:   [chat.turn  ▼]                                   │
│                                                                  │
│ [Cancel]                                            [Save]       │
└──────────────────────────────────────────────────────────────────┘
```

State changes go through `apply_mappings` IPC → Rust regenerates the
collector config → supervisor restarts the collector → user sees a
"applied" toast.

## Collector integration

Two mechanisms feed into collector.yaml:

1. **Hook/watcher mappings** are read by the Rust hook drivers
   (`cursor_common.rs`, `cline_watcher.rs`, wrapper scripts). The
   mapping tells the hook _what to emit_ — the hook constructs the
   right Tier A metric/log shape itself. Collector pipeline is
   unchanged for these.

2. **Native-OTel synthesis** uses the collector's `transform`
   processor. For each harness that has `synthesize-from-native`
   rows, append a transform processor like:

   ```yaml
   transform/tierA-claude_code:
     metric_statements:
       - context: metric
         statements:
           - set(name, "trove.harness.tokens") where name == "claude_code.token.usage"
           # ... etc
   ```

   The Rust supervisor regenerates collector.yaml from
   `MappingState` on every apply.

## Persistence and migration

- `state.json` schema bump v5 → v6. New top-level `mappings:
MappingState`. v5 documents default to an empty `mappings` (or to
  per-harness defaults — implementer's call; either is safe).
- Atomic write via the existing `write_atomic` helper.
- On startup, validate the mapping graph: every `targetMetric` must
  be a known Tier A metric; every `attributes` value must be in the
  enum domain for that metric; no two enabled rules for the same
  harness emit on the same source-event (would double-count).

## Testing strategy

- Rust unit tests: schema round-trips, validator catches double-emit,
  collector.yaml regeneration is deterministic given the same
  `MappingState`.
- TS unit tests: Zod schema accepts the v6 shape; rejects invalid
  enums.
- Integration test: with a synthetic `MappingState`, send a hook
  event through `cursor_common`, assert the emitted OTLP payload
  matches the rule's `metric` + `attributes`.
- E2E (Playwright): open Settings → Metric Mapping, expand cursor,
  edit one row, click Apply, assert the supervisor restarts (state
  channel transition) and the new mapping is persisted to
  `state.json`.

## Open questions for the implementing session

1. **Rate-table updates**: do we ship a "check for updated rates"
   flow now, or rely on Trove releases? Recommend: releases only,
   add a footer note in the UI showing the rate-table version.
2. **Default ON or OFF for native-OTel synthesis?** Trade-off: ON
   means Tier A is populated immediately for every harness, but
   users see _both_ `claude_code.token.usage` and
   `trove.harness.tokens` in their backend. OFF means a clean
   namespace by default but cross-harness dashboards are empty until
   the user opts in. Recommend: ON for `events` and `tokens`,
   OFF for `cost.usd` (cost double-count is more confusing).
3. **What does "Apply changes" actually do** under the hood?
   - (a) write `state.json`, regenerate collector.yaml, signal
     supervisor to reload. Visible 1–3s collector blip.
   - (b) hot-reload via collector's config-reload API (not all
     processors support it). No blip but more complexity.
     Recommend (a) for v1.
4. **Per-user vs per-machine**: state.json today is per-user. Should
   mapping be shareable across a team (e.g., import/export JSON
   snippet)? Recommend: yes, ship an "Export mapping" /
   "Import mapping" button on the settings tab. Trivial, helps
   teams standardize.
5. **What about Tier B passthrough toggle?** Right now Tier B always
   passes through. Should users be able to disable a harness's
   native metrics entirely? Recommend: no — that's an "exclude
   harness from Trove" decision, which already exists in the main
   Harnesses tab.

## Estimated scope

- Rust: ~800 LoC (mapping types, validator, collector regen,
  supervisor wiring).
- TS/React: ~1200 LoC (Settings tab, edit dialog, IPC).
- Tests: ~600 LoC.
- Docs: update `adding-a-harness.md` with "how to declare your
  mapping defaults."

Roughly a focused 2–3 day sprint for a single engineer.

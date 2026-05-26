# Droid (factory.ai) OTLP Integration — Findings & Design Decisions

_Documented May 2026 from live debugging session._

---

## Background

Droid (the `factory.ai` CLI) is a Tier 1 harness in Trove: the UI toggle writes `export` statements into the user's shell RC file so that `OTEL_TELEMETRY_ENDPOINT` is set when Droid runs. This works — Droid does emit OTLP to Trove's local collector.

---

## Critical SDK Quirk: `OTEL_RESOURCE_ATTRIBUTES` is ignored

**factory.ai's SDK does not honour the standard `OTEL_RESOURCE_ATTRIBUTES` environment variable.**

Trove writes the following into the user's shell RC:

```sh
export OTEL_TELEMETRY_ENDPOINT=http://127.0.0.1:4318
export OTEL_RESOURCE_ATTRIBUTES=harness.id=droid,harness.name=Droid,service.name=droid
```

The env var IS present in Droid's process (confirmed via `/proc/<pid>/environ`), but none of `harness.id`, `harness.name`, or `service.name` ever arrive in Droid's OTLP payloads. The factory.ai SDK sets its own resource attributes and ignores the OTel-standard env var.

**Consequence:** All resource-attribute-based filtering and tagging is useless for Droid.  
**Solution:** Use the metric name prefix `droid.*` as the sole discriminator.

---

## What does arrive

- All Droid metrics use the `droid.*` namespace (e.g. `droid.tool.invocations`, `droid.tool.execution_time`, `droid.git.commits`).
- No traces or logs are emitted by Droid's SDK.
- No resource attributes beyond whatever the SDK sets internally (not `harness.id`, not `service.name`).

---

## Collector codegen design

Because of the SDK quirk, two components in `collector/codegen.rs` use metric-name matching instead of resource-attribute matching for Droid:

### `filter/diag-droid`

Uses `metrics.metric` OTTL context (not `metrics.datapoint`), where `name` is the metric name:

```yaml
filter/diag-droid:
  error_mode: ignore
  traces:
    span:
      - 'true'          # drop all — Droid emits no traces
  metrics:
    metric:
      - 'not IsMatch(name, "^droid\\.")'   # drop non-droid metrics; keep droid.*
  logs:
    log_record:
      - 'true'          # drop all — Droid emits no logs
```

> **OTel filter semantics:** a condition evaluating to `true` **drops** the record; `false` passes it through. `'true'` and `'false'` are literal constant expressions. Using `'false'` as a "drop-all" is wrong — it passes everything through.

### `transform/harness-tag`

Adds a `context: metric` block in `metric_statements` that back-fills `harness.id` and `harness.name` on the resource, keyed by metric name:

```yaml
transform/harness-tag:
  metric_statements:
    - context: metric
      statements:
        - 'set(resource.attributes["harness.id"], "droid") where IsMatch(name, "^droid\\.")'
        - 'set(resource.attributes["harness.name"], "Droid") where IsMatch(name, "^droid\\.")'
    - context: resource
      statements:
        - ...  # service.name-based tags for other harnesses
```

`context: metric` in the OTel transform processor exposes:
- `name` — the metric name
- `resource.attributes["key"]` — readable and writable

This runs in the main pipeline so Droid's Tier A metrics in SigNoz carry the correct `harness.id=droid` attribution.

---

## Diag pipeline bleed: a subtle gotcha

All diag pipelines (`metrics/diag-droid`, `logs/diag-claude-code`, etc.) share the same `otlp` receiver. Every incoming OTLP payload enters every diag pipeline. The filter processor is the **only** gate.

If `filter/diag-droid` passed all logs through (because the condition was incorrectly `'false'`), then Claude Code's logs — which arrive via the shared `otlp` receiver — would pass through `filter/diag-droid` and increment its `outgoing_items` counter, making them appear as Droid log activity in the FlowChart.

---

## Debugging tools

**`scripts/otlp-tap.py`** — a minimal HTTP server that accepts OTLP/HTTP payloads on port 4319 and prints readable strings extracted from the binary protobuf. No third-party dependencies.

```sh
# Terminal 1
python3 scripts/otlp-tap.py

# Terminal 2 — point Droid at the tap instead of Trove
export OTEL_TELEMETRY_ENDPOINT=http://127.0.0.1:4319
droid
# Wait ~60 s for Droid's flush interval, then check Terminal 1.
```

**Prometheus internal metrics** at `http://127.0.0.1:8888/metrics`:
- `otelcol_processor_incoming_items{processor="filter/diag-droid"}` — total records entering the filter
- `otelcol_processor_outgoing_items{processor="filter/diag-droid"}` — records passing the filter (= Droid-attributed records)
- `otelcol_processor_outgoing_items{processor="metricstransform/tierA-droid"}` — confirms `droid.*` metrics are being received and Tier A copies inserted

---

## `native_service_name_candidates` and why Droid returns `&["droid"]`

The function returns a non-empty slice for Droid even though `service.name=droid` never arrives. This keeps Droid included in `tag_harnesses` and `diag_harnesses` (both filtered by `!candidates.is_empty()`). The actual tagging and filtering in `build_harness_tag_block` and `apply_diag_pipelines` special-case Droid to use name-based matching instead. The `"droid"` value in the slice is never used as a filter key.

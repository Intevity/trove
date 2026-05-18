# Harness × platform results matrix

Companion to [`AUTOMATED_TESTING_PLAN.md`](AUTOMATED_TESTING_PLAN.md). One
row per (harness, platform, date) test run. Each cell records two signals
per §4 of the plan:

- **Receipt:** Trove's bundled collector accepted an OTLP batch tagged with
  `harness.id=<id>`. Pulled from Trove UI → Diagnostics → Collector logs.
- **Query:** The platform's read API returned ≥1 row for a recent query
  filtered on `harness.id`.

Values: `PASS`, `FAIL`, `SKIP`, or `—` (not yet attempted).

## Status grid

|                    | grafana-local  | openobserve-local | hyperdx-local  | signoz-local   | opensearch-local | elastic-local | sentry-local | signoz-cloud | honeycomb | grafana-cloud | datadog | new-relic | splunk-obs | dynatrace | elastic-cloud | clickstack-cloud | chronosphere | sentry-saas |
| ------------------ | -------------- | ----------------- | -------------- | -------------- | ---------------- | ------------- | ------------ | ------------ | --------- | ------------- | ------- | --------- | ---------- | --------- | ------------- | ---------------- | ------------ | ----------- |
| **claude-code**    | R:PASS Q:PASS  | R:PASS Q:PASS     | R:PASS Q:PASS  | R:FAIL Q:FAIL‡ | R:PASS Q:PASS    | —             | —            | —            | —         | —             | —       | —         | —          | —         | —             | —                | —            | —           |
| **gemini-cli**     | R:PASS Q:PASS  | R:PASS Q:PASS     | R:PASS Q:PASS  | R:FAIL Q:FAIL‡ | R:PASS Q:PASS    | —             | —            | —            | —         | —             | —       | —         | —          | —         | —             | —                | —            | —           |
| **cursor-cli**     | R:FAIL Q:FAIL† | R:FAIL Q:FAIL†    | R:FAIL Q:FAIL† | R:FAIL Q:FAIL† | R:FAIL Q:FAIL†   | —             | —            | —            | —         | —             | —       | —         | —          | —         | —             | —                | —            | —           |
| **claude-desktop** | —              | —                 | —              | —              | —                | —             | —            | —            | —         | —             | —       | —         | —          | —         | —             | —                | —            | —           |
| **cursor-ide**     | —              | —                 | —              | —              | —                | —             | —            | —            | —         | —             | —       | —         | —          | —         | —             | —                | —            | —           |
| **cline**          | —              | —                 | —              | —              | —                | —             | —            | —            | —         | —             | —       | —         | —          | —         | —             | —                | —            | —           |
| **codex-cli**      | —              | —                 | —              | —              | —                | —             | —            | —            | —         | —             | —       | —         | —          | —         | —             | —                | —            | —           |
| **qwen-code**      | —              | —                 | —              | —              | —                | —             | —            | —            | —         | —             | —       | —         | —          | —         | —             | —                | —            | —           |
| **opencode**       | —              | —                 | —              | —              | —                | —             | —            | —            | —         | —             | —       | —         | —          | —         | —             | —                | —            | —           |
| **aider**          | —              | —                 | —              | —              | —                | —             | —            | —            | —         | —             | —       | —         | —          | —         | —             | —                | —            | —           |
| **copilot-cli**    | —              | —                 | —              | —              | —                | —             | —            | —            | —         | —             | —       | —         | —          | —         | —             | —                | —            | —           |

Format in each cell once tested: `R:PASS Q:PASS` / `R:PASS Q:FAIL` / etc.
Drill down to the dated runs below.

## Run log

Each entry follows this template — append at the bottom; never edit older
entries.

```
### YYYY-MM-DD — <harness id> × <platform id>

- **Trove preset config:** <preset id + any custom attrs>
- **Smoke command:** <exact shell line>
- **Receipt:**
  - Log line: `…accepted N spans|metrics|logs…`
  - harness.id observed: `<id>`
  - Result: PASS / FAIL
- **Query:**
  - Body: `<PromQL / DSL / API call>`
  - Count returned: N
  - Sample row: `<one line>`
  - Result: PASS / FAIL
- **Notes / follow-up:** <issue link if FAIL>
```

### 2026-05-18 — `smoke` × openobserve-local

- **Trove preset config:** n/a (stack-only smoke, no Trove in the loop)
- **Smoke command:** `./scripts/smoke.sh` (synthetic OTLP HTTP POST to `http://localhost:5080/api/default/v1/traces`)
- **Receipt:**
  - HTTP 200 from OpenObserve OTLP intake
  - Result: PASS
- **Query:**
  - Body: `GET /api/default/_search?type=traces` filtered by `trace_id`
  - Count returned: 1
  - Result: PASS
- **Notes / follow-up:** Healthcheck removed from compose (image is distroless, no wget). `.env.example` updated to use a TLD-valid email — OpenObserve rejects `admin@local`.

### 2026-05-18 — `smoke` × hyperdx-local (PARTIAL)

- **Trove preset config:** n/a (stack-only smoke, no Trove in the loop)
- **Smoke command:** Stack-health probes (UI, ClickHouse, container netstat)
- **Receipt:**
  - HyperDX UI at :8080: HTTP 302 (login redirect) — PASS
  - ClickHouse HTTP at :18123: responds, auth-protected — PASS (stack healthy)
  - Container ports 4317/4318: **not listening** until team setup in the UI completes
  - Result: PARTIAL — stack boots; OTLP receivers gated behind onboarding
- **Query:** deferred until OTLP intake is unlocked
- **Notes / follow-up:** The OSS `hyperdx-all-in-one:2` image starts the OTel collector in a "wait-for-config" state. Need to (a) open http://localhost:8080, (b) create the admin account, (c) generate an Ingestion API Key, and (d) re-run `INGESTION_KEY=<key> ./scripts/smoke.sh`. This will be done as part of the Phase D harness pairing.

### 2026-05-18 — `smoke` × signoz-local (PARTIAL)

- **Trove preset config:** n/a (stack-only smoke, no Trove in the loop)
- **Smoke command:** stack-health probes via vendored upstream compose
- **Receipt:**
  - All 4 services healthy: `signoz`, `signoz-clickhouse`, `signoz-otel-collector`, `signoz-zookeeper-1`
  - SigNoz UI at :8080: HTTP 302 (login redirect) — PASS
  - OTLP HTTP at :4318: connection reset — collector's OTLP receivers are not bound
  - Backend logs: `cannot create agent without orgId` (otel-collector polls OpAMP, server rejects until an org exists)
  - Result: PARTIAL — stack boots; OTLP receivers gated behind admin onboarding
- **Query:** deferred
- **Notes / follow-up:**
  - Switched `signoz-local/` from a hand-rolled compose to a thin vendor wrapper (`./scripts/install.sh` clones `SigNoz/signoz@v0.124.0` to `vendor/signoz/`); the upstream stack needs ClickHouse cluster XMLs + otel-collector OpAMP configs that are infeasible to reproduce inline.
  - Phase D unblocker: open http://localhost:8080, create admin account, then re-run `./scripts/smoke.sh`. The collector should bind OTLP receivers within ~30s of the org being created.

### 2026-05-18 — `smoke` × opensearch-local

- **Trove preset config:** n/a (stack-only smoke, no Trove in the loop)
- **Smoke command:** `./scripts/smoke.sh` (OTLP HTTP → otel-collector @ :14326 → Data Prepper @ :21890 gRPC → OpenSearch `otel-v1-apm-span-*`)
- **Receipt:**
  - HTTP 200 from front-door otel-collector OTLP intake
  - Result: PASS
- **Query:**
  - Body: `GET /otel-v1-apm-span-*/_count?q=traceId:<id>` via OpenSearch HTTP
  - Count returned: 1
  - Result: PASS
- **Notes / follow-up:**
  - Initial run failed because the trace pipeline never built — the `otel-service-map-pipeline` referenced `otel-trace-pipeline` and Data Prepper 2.10 throws a `NullPointerException` building cross-pipeline connectors in that order. Removed the service-map pipeline; smoke still proves trace ingest.
  - Data Prepper's Trace Raw Processor has a hard-coded **180s flush interval**. Updated `scripts/smoke.sh` to poll for up to 200s instead of failing after 8s.
  - Phase D will need the same long-wait awareness when verifying Trove traces against this stack.

### 2026-05-18 — `claude-code` × openobserve-local

- **Trove preset config:** OpenObserve, endpoint `http://localhost:5080`, org `default`, Basic auth from local stack creds. All three configured platforms (signoz, grafana, openobserve) are enabled simultaneously — Trove fans every signal out to all three.
- **Smoke command:** `claude -p "say hi" --output-format text` (one turn, "Hi!" response)
- **Receipt:**
  - Source: `~/Library/Logs/com.intevity.trove/collector.log` after the turn
  - Lines: `"resource logs":1, "log records":8` and `"resource metrics":1, "metrics":3, "data points":6` on the `debug/diag-noop` exporter (per-harness counter pipeline)
  - Main pipeline fanned out to `[otlp/signoz-31fb8e0a, otlphttp/grafana-42520ad9, otlphttp/openobserve-93eb10f1]`
  - Result: PASS
- **Query:**
  - Body: `POST /api/default/_search?type=logs` with `SELECT * FROM "default" WHERE harness_id = 'claude-code'`
  - Count returned: ≥1 (limit 5 returned hit on `mcp_server_connection` log with full claude-code resource attrs incl. `service_name=claude-code`, `session_id`, `user_email`)
  - Streams auto-created on first ingest: `claude_code_cost_usage`, `claude_code_session_count`, …
  - Result: PASS
- **Notes / follow-up:**
  - Initial misread: I saw the grafana exporter's retry errors in the log and thought OpenObserve wasn't wired. The rendered collector config at `~/Library/Application Support/com.intevity.trove/collector.yaml` clearly fans out to all 3 enabled platforms; the grafana exports were just loud because grafana-local is currently stopped.
  - Useful query shape for future Trove → OpenObserve verification:
    `SELECT * FROM "default" WHERE harness_id = '<id>' AND _timestamp > <start_micros>`

### 2026-05-18 — `gemini-cli` × openobserve-local

- **Trove preset config:** same as above (3 platforms enabled, openobserve in the fan-out).
- **Smoke command:** `gemini --prompt "say hi" --skip-trust` (one turn, model replied "Hi! How can I help you with your project today?")
- **Receipt:**
  - `debug/diag-noop` exporter saw: 12 log records across two batches, 1 span, 10 metrics / 23 data points.
  - Result: PASS
- **Query:**
  - Body: `SELECT service_name, harness_id, event_name FROM "default" WHERE harness_id = 'gemini-cli'`
  - Top hits: `gemini_cli.plan.approval_mode_duration`, `gemini_cli.api_response`, `gen_ai.client.inference.operation.details`
  - Result: PASS
- **Notes:** Gemini CLI refuses to operate in an untrusted dir without `--skip-trust`. First call was rejected before any model interaction but Trove still received a small bundle of startup telemetry (3 metrics, 4 logs). Real-turn verification needed the `--skip-trust` flag.

### 2026-05-18 — `cursor-cli` × openobserve-local († FAIL — integration gap)

- **Trove preset config:** same as above.
- **Smoke command:** `cursor-agent -p "say hi"` (authenticated; one turn — got `"Hi Jeff — good to meet you. What would you like to work on in the Trove repo today?"`)
- **Receipt:** none. Trove collector log saw zero new lines during/after the turn.
- **Query:** OpenObserve has zero rows for `harness_id LIKE 'cursor%'`.
- **Diagnosis:** Trove's Cursor adapter installs hooks into `~/.cursor/hooks.json` (`beforeShellExecution`, `afterShellExecution`, `beforeSubmitPrompt`, `afterAgentResponse`). `cursor-agent --help` makes no mention of hooks, and the hook script is not invoked during a CLI turn. The hook system appears to be **IDE-only**. The `cursor-cli` adapter (`packages/app/src-tauri/src/adapters/cursor_cli.rs`) and `cursor-ide` adapter share the same hooks.json patch, but only IDE invocations actually trigger the patched events.
- **Suggested follow-up:** Decide whether `cursor-cli` should:
  1. Be removed from the harness list (until Cursor adds hook support to the CLI), OR
  2. Use a different capture mechanism (e.g., a shell wrapper or PATH shim like the aider/copilot-cli adapters do), OR
  3. Stay as-is but be documented as IDE-coverage-only in the UI tooltip.
- **Workaround for end-to-end testing:** Use Cursor IDE (Jeff-driven) when verifying Cursor coverage — pairing falls in §1b of the testing plan.

### 2026-05-18 — three CLIs × grafana-local

- **Trove preset config:** Grafana, endpoint `http://localhost:14318`, Authorization `Bearer <rotated 32-byte hex>`. Token had to be rotated — operator's previous setup used the literal placeholder `changeme-…` and Trove preset value drifted from it, causing all signals to drop with HTTP 401 (`provided authorization does not match expected scheme or token`). After rotating + restarting the otel-collector + re-pasting `Bearer <token>` in Trove, all forwards succeeded.
- **claude-code:**
  - Receipt: PASS (8 log records, 3 metrics/6 data points at Trove's diag-noop exporter; main-pipeline fan-out succeeded).
  - Query: PASS. Loki `{service_name="claude-code"}` last 5 min → 8 records. Prometheus `{harness_id="claude-code"}` → `target_info` series with samples. (Counter metrics like `claude_code_session_count_total` only flush every 60s per `OTEL_METRIC_EXPORT_INTERVAL`; a longer-lived session would expose them.)
- **gemini-cli:**
  - Receipt: PASS (12 log records, 1 span, 5 metrics/13 data points at Trove level).
  - Query: PASS. Loki `{service_name="gemini-cli"}` → 12 records. Tempo direct-API search `tags=harness.id=gemini-cli` → trace `2f4b006015cae82a72609a675f861a06` (`llm_call`, 24 s duration, `rootServiceName=gemini-cli`).
- **cursor-cli:**
  - Receipt: FAIL (no new collector lines for a `cursor`-scoped resource — same as openobserve run).
  - Query: FAIL. Loki has no `service_name~"cursor.*"` series. Prometheus shows `harness_id="cursor"` only on Trove's internal `trove_harness_events_total` (housekeeping counter, not actual harness telemetry). Confirms the IDE-hooks-only diagnosis from the openobserve run.
- **Trove behaviour observation worth noting in release notes:** When a forwarding endpoint rejects auth (HTTP 401), Trove's collector logs `Permanent error → Dropping data, dropped_items=N`. The harness-side OTel SDK still considers the export successful because Trove's own OTLP intake returned 200. Result: a stale-credential platform silently black-holes data. Trove UI should surface this state (or the per-platform diagnostic counter should reflect drop volume).

### 2026-05-18 — three CLIs × opensearch-local

- **Trove preset config:** OpenSearch, endpoint `http://localhost:14326`, Authorization `Basic ZGV2OmRldg==` (placeholder — Data Prepper accepts unauthenticated, see plan §2). Now 4 platforms enabled simultaneously in Trove (signoz, grafana, openobserve, opensearch); the fan-out pipeline includes all four.
- **claude-code:** Receipt PASS (8 logs / 3 metrics at diag-noop). Query PASS in `logs-otel-v1-*` (`serviceName:"claude-code"` → 8 docs incl. `claude_code.mcp_server_connection` body), `metrics-otel-v1-*` → 16 docs (e.g. `claude_code.session.count` with `value:1.0`).
- **gemini-cli:** Receipt PASS (12 logs / 1 span / 10 metrics at diag-noop). Query PASS in `logs-otel-v1-*` → 12 docs, `metrics-otel-v1-*` → 30 docs, `otel-v1-apm-span-*` → 1 span (`name:llm_call`, `serviceName:gemini-cli`).
- **cursor-cli:** Receipt FAIL / Query FAIL — third independent confirmation of the IDE-hooks-only gap.
- **Notes:**
  - No `otlphttp/opensearch-*` retry-sender errors in the collector log — confirms forwards succeeded silently.
  - Field-name quirk: Data Prepper rewrites `.` to `@` inside attribute keys (`resource.attributes.harness@id`, `log.attributes.user@email`, etc.). For queries, use the synthesised `serviceName` top-level field rather than re-traversing the nested resource path — much simpler.
  - Indexed extra: `otel-v1-apm-span-*` also contained Trove's own `trove-test-export` self-test span; harness-attributed spans were just the one gemini span (claude-code emits no traces).

### 2026-05-18 — three CLIs × hyperdx-local

- **Trove preset config:** ClickStack, endpoint `http://localhost:14324`, ingestion key from HyperDX UI. UI port moved from default 8080 → host **18080** in `observability/hyperdx-local/docker-compose.yml` because operator had other localhost forwards on 80xx. 5 platforms now in Trove's fan-out pipeline.
- **claude-code:** Receipt PASS / Query PASS. ClickHouse `default.otel_logs` → 8 records `WHERE ServiceName='claude-code'`. `default.otel_metrics_sum` → 15 data points.
- **gemini-cli:** Receipt PASS / Query PASS. ClickHouse `default.otel_logs` → 12 records. `default.otel_metrics_sum` → 10 data points. `default.otel_traces` → 1 span `llm_call`.
- **cursor-cli:** Receipt FAIL / Query FAIL — IDE-hooks gap (4th confirmation).
- **Notes:**
  - HyperDX UI flow gates OTLP intake: collector receivers don't bind until the bundled OpAMP server hands the collector a config tied to a real user/team. Smoke-only test from §C2 (PARTIAL) flipped to full PASS once operator did the signup.
  - HyperDX uses the bundled ClickHouse's `default` DB (not `otel` as I initially guessed). Schema: `otel_logs`, `otel_traces`, `otel_metrics_*` tables. Field `Timestamp` (not `TimestampTime`).
  - HyperDX's all-in-one image self-emits to its own collector (`ServiceName='otelcol-hyperdx'`, 322 metric points in the same window) — useful to filter out when running cross-harness queries.

### 2026-05-18 — three CLIs × signoz-local (‡ TLS-mismatch FAIL — Trove gap)

- **Trove preset config:** SigNoz, endpoint `localhost:14317` (gRPC), placeholder ingestion key. SigNoz vendored at `v0.124.0`; UI/OTLP host ports remapped to **13301 / 14317 / 14318** in `vendor/signoz/deploy/docker/docker-compose.yaml` (`scripts/install.sh` now patches these on clone). 5 platforms (signoz, grafana, openobserve, opensearch, clickstack) in Trove's fan-out pipeline.
- **claude-code, gemini-cli, cursor-cli:** Receipt PASS at Trove level (same shape as every other run — claude 8 logs / 3 metrics, gemini 12 logs / 1 span / 10 metrics, cursor 0). Forwarding to signoz-local **FAILED** on every signal for every harness with:
  - `rpc error: code = Unavailable desc = connection error: desc = "transport: authentication handshake failed: tls: first record does not look like a TLS handshake"`
- **Diagnosis (real Trove gap):** Trove's SigNoz preset emits a collector exporter with `tls: insecure: false` hardcoded:
  - `packages/collector-presets/templates/signoz.yaml:38-39` (reference template)
  - Inline rendering in `packages/app/src-tauri/src/collector/codegen.rs` (Backend::Signoz branch)
  - The Trove UI exposes no per-preset TLS toggle and no auto-detection (e.g., "if endpoint host is loopback, set insecure: true").
  - This means the SigNoz preset **only works against SigNoz Cloud and any self-hosted SigNoz that terminates TLS in front of the gRPC port**. A vanilla `docker compose up` of SigNoz OSS — which serves plaintext gRPC on 4317 — cannot be paired with Trove today.
- **Suggested fix options (for Trove release):**
  1. Add a `tls.insecure: bool` field to the `Backend::Signoz` schema, exposed in the UI as "Use plaintext (self-hosted)" — defaulting off to match the Cloud-first preset.
  2. Auto-detect: if the user-entered endpoint host resolves to a loopback address (`localhost`, `127.0.0.1`, `::1`), render `tls: insecure: true`.
  3. Bundle a TLS-terminating sidecar in `signoz-local/` (works as a workaround for testing but doesn't fix the preset; mostly useful for verifying the rest of Trove behaves correctly downstream of the export).
- **Workaround attempted in this session:** none — recorded as FAIL with the diagnostic above so the fix can land separately.

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

|                    | grafana-local | openobserve-local | hyperdx-local | signoz-local   | opensearch-local | elastic-local | sentry-local | signoz-cloud | honeycomb | grafana-cloud | datadog | new-relic | splunk-obs | dynatrace | elastic-cloud | clickstack-cloud | chronosphere | sentry-saas |
| ------------------ | ------------- | ----------------- | ------------- | -------------- | ---------------- | ------------- | ------------ | ------------ | --------- | ------------- | ------- | --------- | ---------- | --------- | ------------- | ---------------- | ------------ | ----------- |
| **claude-code**    | R:PASS Q:PASS | R:PASS Q:PASS     | R:PASS Q:PASS | R:PASS Q:PASS§ | R:PASS Q:PASS    | —             | —            | —            | —         | —             | —       | —         | —          | —         | —             | —                | —            | —           |
| **gemini-cli**     | R:PASS Q:PASS | R:PASS Q:PASS     | R:PASS Q:PASS | R:PASS Q:PASS§ | R:PASS Q:PASS    | —             | —            | —            | —         | —             | —       | —         | —          | —         | —             | —                | —            | —           |
| **cursor-cli**     | —             | R:PASS Q:PASS¶    | —             | R:PASS Q:PASS¶ | —                | —             | —            | —            | —         | —             | —       | —         | —          | —         | —             | —                | —            | —           |
| **claude-desktop** | —             | —                 | —             | —              | —                | —             | —            | —            | —         | —             | —       | —         | —          | —         | —             | —                | —            | —           |
| **cursor-ide**     | —             | —                 | —             | —              | —                | —             | —            | —            | —         | —             | —       | —         | —          | —         | —             | —                | —            | —           |
| **cline**          | —             | —                 | —             | —              | —                | —             | —            | —            | —         | —             | —       | —         | —          | —         | —             | —                | —            | —           |
| **codex-cli**      | —             | —                 | —             | —              | —                | —             | —            | —            | —         | —             | —       | —         | —          | —         | —             | —                | —            | —           |
| **codex-desktop**¤ | —             | —                 | —             | —              | —                | —             | —            | —            | —         | —             | —       | —         | —          | —         | —             | —                | —            | —           |
| **qwen-code**      | —             | R:PASS Q:PASS◇    | —             | R:PASS Q:PASS◇ | —                | —             | —            | —            | —         | —             | —       | —         | —          | —         | —             | —                | —            | —           |
| **opencode**       | —             | R:PASS Q:PASS◇    | —             | R:PASS Q:PASS◇ | —                | —             | —            | —            | —         | —             | —       | —         | —          | —         | —             | —                | —            | —           |
| **aider**          | —             | R:PASS Q:PASS◇    | —             | R:PASS Q:PASS◇ | —                | —             | —            | —            | —         | —             | —       | —         | —          | —         | —             | —                | —            | —           |
| **copilot-cli**    | —             | R:PASS Q:PASS◇    | —             | R:PASS Q:PASS◇ | —                | —             | —            | —            | —         | —             | —       | —         | —          | —         | —             | —                | —            | —           |

Format in each cell once tested: `R:PASS Q:PASS` / `R:PASS Q:FAIL` / etc.

Footnotes:

- `†` Cursor-CLI IDE-hooks gap (historical, before Fix 2). The `cursor-cli` rows were re-evaluated after Fix 2 — only the (cursor-cli × {openobserve, signoz}) pairs have been re-run; the other three columns are blank pending a fresh test pass.
- `‡` SigNoz TLS-mismatch (historical, before Fix 1).
- `§` Post-fix re-run validating Fix 1 (SigNoz loopback auto-detect TLS insecure).
- `¶` Post-fix re-run validating Fix 2 (cursor-cli shell-function wrapper, replaces the IDE-hooks path).
- `◇` 2026-05-18 autonomous validation pass for newly installed harnesses (opencode, qwen-code, aider, copilot-cli). PASS on telemetry signals; the run also surfaced four UI-side adapter bugs (Bugs A–D below) that did not block telemetry but blocked the in-app enable flow. See run log.
- `¤` `codex-desktop` and `codex-cli` share `~/.codex/config.toml` — both invoke the same Rust `codex app-server` backend, so a single managed `[otel.*]` block instruments both rows. The fence header carries `deps=codex-cli,codex-desktop` so each row enables/disables independently; the block is only stripped when the last dep is removed. Either row's enablement produces backend telemetry tagged `service.name = codex` (or `codex-cli` with env overrides); the existing codex-cli mapping rules consume both.

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

### 2026-05-18 — three CLIs × signoz-local + openobserve-local (§ Fix 1 + ¶ Fix 2 — post-fix re-run)

- **Trove preset config:** Identical to prior runs — SigNoz `localhost:14317` (gRPC), OpenObserve `localhost:5080` (HTTP), plus grafana/opensearch/clickstack in fan-out. Rebuilt Trove from branch `fix/release-pairing-findings` (Fix 1 + Fix 2 applied).
- **Fix 1 — SigNoz TLS loopback auto-detect (`§`):** Replaced hardcoded `tls.insecure: false` in `packages/app/src-tauri/src/collector/codegen.rs` (Backend::Signoz branch) with a runtime check `endpoint_is_loopback(endpoint)` that strips scheme + path, handles bracketed IPv6, and returns true for `localhost`, `127.0.0.1`, `::1`, `0.0.0.0`. Generated `collector.yaml` for this run shows `otlp/signoz-31fb8e0a: tls: insecure: true` exactly as expected for a loopback endpoint; Cloud endpoints (e.g. `ingest.us.signoz.cloud:443`) still resolve to `insecure: false`.
- **Fix 2 — cursor-cli wrapper (`¶`):** Replaced the Cursor IDE hooks-only path with a `wrapper_common`-pattern shell function. New artifacts:
  - `resources/wrappers/trove-cursor-agent` — bash wrapper that re-execs the real `cursor-agent`, then JSON-line-logs `{ts, tool=cursor-cli, argc, exit_code, duration_ms}` to `~/.local/state/trove/cursor-agent.log`.
  - `packages/app/src-tauri/src/adapters/cursor_cli.rs` — full rewrite mirroring `copilot_cli.rs`: managed block in `~/.zshrc` adds a `cursor-agent()` shell function pointing at the wrapper; `parse_event_line` emits OTLP `LogRecord` with `service.name=cursor-cli`, `harness.id=cursor-cli`, `harness.name=Cursor CLI`.
  - `packages/app/src-tauri/src/ipc/commands.rs` — separated `CursorIde | CursorCli` arms; `spawn_tier3_watcher` now spawns a `wrapper_log_watcher` for cursor-cli that tails the wrapper log.
- **Smoke commands:** `claude -p 'say hi…'`, `gemini --skip-trust --prompt 'say hi…'`, `<wrapper> --yolo -p 'say hi…'`.
- **Receipt (Trove collector):** No retry/handshake errors for `otlp/signoz-31fb8e0a` exporter at any point during the run (contrast prior run, where every batch failed with TLS handshake). Errors for `otlphttp/opensearch-…` and `otlphttp/clickstack-…` remain (those stacks are down — expected).
- **Query — SigNoz (ClickHouse direct):**
  ```sql
  SELECT resources_string['service.name'] AS svc, count() AS n, max(timestamp) AS latest_ns
  FROM signoz_logs.distributed_logs_v2
  WHERE timestamp >= (toUnixTimestamp(now() - toIntervalMinute(10)) * 1000000000)
  GROUP BY svc ORDER BY latest_ns DESC
  ```
  Returned: `cursor-cli: 6`, `gemini-cli: 32`, `claude-code: 10` — all three harnesses landed in SigNoz OSS via plaintext gRPC. **Fix 1 PASS.**
- **Query — OpenObserve:** `SELECT _timestamp, service_name, body FROM default WHERE service_name = 'cursor-cli' ORDER BY _timestamp DESC LIMIT 5` returned 3 rows at the three wrapper-invocation timestamps. **Fix 2 PASS.**
- **Notes / follow-up:**
  - Fix 1 is a behavior change — Cloud SigNoz users see no diff (still TLS-on); self-hosters get a working out-of-the-box experience. Five unit tests in `collector/codegen.rs` cover the matrix.
  - Fix 2's transform processor in `collector.yaml` does not yet include a `service.name == cursor-cli → harness.id = cursor-cli` rule (the adapter sets the resource attr directly when crafting the OTLP, so this is cosmetic; queries by `harness.id` will still find cursor-cli telemetry). Add the transform-rule entry to `packages/app/src-tauri/src/collector/codegen.rs` harness-tag block before release for parity with other Tier-1 adapters.
  - A latent UI bug exists for cursor-cli: clicking "Disable" produces only a brief jitter and does not persist. Worked around in this session by hand-editing `state.json` to add a `cursor-cli` entry with `trovePatch.format: yaml`. Filed as a follow-up for the next pass.

### 2026-05-18 — Fix 3 — per-platform health pill (validated live in Platforms tab)

- **Branch state:** `fix/release-pairing-findings` on top of Fixes 1 + 2.
- **Goal:** make silent data loss visible. Before this fix, when one destination's exporter started failing (stale creds, wrong endpoint, container down) the row still said "Enabled", the global tray icon stayed green (other destinations still received), and batches piled into the retry queue silently. Now each destination row in the Platforms tab carries a colored status dot (gray / green / amber / red), and a click expands an inline detail panel with the last error string and 60-second window counters.
- **Architecture:**
  - **Color signal (counters, push):** Extended the 5 s Prometheus scrape in `packages/app/src-tauri/src/collector/metrics_tap.rs` to parse `otelcol_exporter_sent_*` / `otelcol_exporter_send_failed_*` time series keyed by the `{exporter="..."}` label (`parse_per_exporter_counts` + `ExporterCounts`). Each scrape tick's deltas land in a per-backend rolling 60 s window (`BackendHealthSamples::observe` in `collector/derive.rs`).
  - **Tooltip + Red-state signal (stderr, push):** A best-effort JSON-ish parser (`try_parse_exporter_error_line` in `collector/logs.rs`) reads every line on the existing supervisor log broadcast and pulls out `(otelcol.component.id, error)` from records where `otelcol.component.kind == "exporter"`. Real-world wrinkle caught in dev validation: the Go logger emits `"key": "value"` with a space after `:`, so the parser tolerates both `"k":"v"` and `"k": "v"` JSON spacing.
  - **Why both signals:** the collector's `_send_failed_*` counter only increments when retries are permanently dropped. An exporter stuck in retry-limbo (the steady state for a wrong endpoint) has `sent=0, failed=0` counters and is invisible to the counter path alone. The derive function flips the row to **Red** if there's a recent stderr error and no recent successes — which is exactly the case we want to catch.
  - **State holder:** `BackendHealthTracker` (`collector/health_tracker.rs`) fuses both inputs into a single `Vec<BackendHealth>` published on a `watch` channel. Wired into `lib.rs` between the metrics tap and the supervisor; held as Tauri-managed state.
  - **Component-id ↔ backend-id mapping:** `backend_id_from_component_id` in `collector/codegen.rs` inverts `env_suffix_for` so a label like `otlphttp/opensearch-a7f0880b` resolves back to backend UUID `a7f0880b-…`.
  - **IPC + UI:** `EVENT_BACKEND_HEALTH` Tauri event (debounced 250 ms in `ipc/collector_status.rs`) + `get_backend_health` command for initial fetch. Frontend: new `useBackendHealth()` hook + new `BackendInstanceRow` / `BackendHealthDetail` components in `PlatformsTab.tsx`; status mapped to existing `StatusDot` (gray/green/amber/red).
- **Truth-table derive function (`collector/derive.rs`):**
  - `(window_sent=0, has_failures=false)` ⇒ Gray
  - `(window_sent>0, has_failures=false)` ⇒ Green (pulses)
  - `(window_sent>0, has_failures=true)` ⇒ Amber
  - `(window_sent=0, has_failures=true)` ⇒ Red
  - `has_failures` is `window_failed > 0` **OR** `last_error_at` within the rolling window.
- **Live validation:** 5 destinations configured — signoz/openobserve/grafana-local up, opensearch/clickstack down. After `claude -p "say hi"` triggered a fan-out:
  - signoz / grafana / openobserve rows: **Green** (`window_sent=5, window_failed=0`, pulsing dot).
  - opensearch / clickstack rows: **Red** (`window_sent=0, window_failed=0` because retries haven't been dropped yet; stderr fallback path fires `recorded stderr error` repeatedly from the retry-sender Go logger).
  - Clicking a red row inline-expands: status pill (`red`), `Sent in last 60s: 0`, `Failed in last 60s: 0`, `Last error: Ns ago`, and the verbatim error string `failed to make an HTTP request: Post "http://localhost:14326/v1/logs": dial tcp [::1]:14326: connect: connection refused`.
- **Tests:** 622 → **623 Rust lib tests pass** (4 codegen-resolver, 5 metrics_tap per-exporter + delta-clamp, 11 derive truth-table + edge cases incl. counter-reset and stale-error-out-of-window, 7 logs parser incl. real captured retry-sender line, 3 health_tracker integration). **365 frontend tests pass.**
- **Pitfalls captured (so future-me doesn't relearn):**
  - The `@trove/shared` package is consumed via its built `dist/` — schema edits don't take effect until `pnpm --filter @trove/shared build`. A stale dist silently rejects every event payload at `safeParse` and the UI permanently shows Gray.
  - Zod's `z.string().datetime()` only accepts 0/3/6/9-digit fractional seconds; chrono's RFC3339 output can be any precision. Use plain `z.string()` for chrono-sourced timestamps unless the producer is pinned.
  - The OTel Go logger writes `"key": "value"` (space after colon) in structured records — a parser pinned to `"key":"value"` will silently match zero real-world lines.

### 2026-05-18 — opencode + qwen-code + aider + copilot-cli × signoz-local + openobserve-local (◇ autonomous validation pass)

- **Trove preset config:** SigNoz `localhost:14317` + OpenObserve `localhost:5080` enabled (grafana/opensearch/clickstack also in fan-out but the latter two stacks were down). Build: dev `pnpm tauri dev` on `fix/release-pairing-findings` post-Fix-3.
- **Telemetry result:** PASS for all four harnesses on both queryable platforms.
- **Per-harness summary:**
  - **opencode (Tier-1, plugin):** Smoke `opencode run "say hi briefly"`. Receipt PASS via the `@devtheops/opencode-plugin-otel` plugin shipped in `~/.config/opencode/opencode.json`. SigNoz: 8 rows `service.name=opencode` in the last 10 min. OpenObserve: 4 rows.
  - **qwen-code (Tier-1, env-var patch):** Smoke `qwen --prompt "say hi briefly" --yolo`. Receipt PASS — Trove's gemini-cli-style settings.json patch in `~/.qwen/settings.json` enables qwen's built-in OTel exporter. SigNoz: 12 rows `service.name=qwen-code`. OpenObserve: 6 rows.
  - **aider (Tier-3, wrapper):** Smoke `aider --sonnet --message "say hi briefly" --yes --no-auto-commits --no-stream` in a fresh `zsh -i -c …`. Wrapper log `~/.local/state/trove/aider.log` got a new line `{"ts":"…","tool":"aider","argc":9,"exit_code":0,"duration_ms":16465}`; Trove's wrapper-log-watcher converted to OTLP. SigNoz: 2 rows. OpenObserve: 1 row.
  - **copilot-cli (Tier-3, wrapper):** Smoke `gh-copilot explain 'ls -la'`. Same wrapper-log path → OTLP. SigNoz: 4+ rows `service.name=copilot-cli`. OpenObserve: 1 row.
- **Bugs surfaced (release blockers — file before tagging):**
  - **Bug A — `build_conflict_payload` validates `~/.zshrc` as YAML.** `ipc/commands.rs:424` builds a conflict-detection payload by calling `extract_region(Format::Yaml, …)` on the shell rc; `validate_yaml` (`safety/sentinels.rs:654`) runs `serde_yml::from_str` and fails with `deserializing from YAML containing more than one document is not supported` (a typical zshrc has `# ---` separators or just multi-statement content). User-facing: clicking **Enable** on aider/copilot-cli when an existing wrapper-style managed block is detected in `~/.zshrc` errors with `/Users/jeff/.zshrc: malformed yaml document`. Fix direction: introduce a `Format::Shell` variant whose validator either no-ops or runs a `bash -n` syntax check, and route wrapper adapters through it instead of `Format::Yaml`. Workaround for this validation pass: manually removed the prior wrapper block from `~/.zshrc` so no conflict-detection fired.
  - **Bug B — `wrapper_common::upsert_managed_block` only supports one managed block per shell rc.** `wrapper_common.rs:216` finds-and-replaces a single fenced `# trove:start` / `# trove:end` region; enabling a second wrapper adapter overwrites the function definition of the first. Net effect: aider + copilot-cli + cursor-cli cannot coexist; only the most-recently-enabled wrapper actually works. Fix direction: switch the fence to be per-adapter (e.g. `# trove-aider:start` / `# trove-aider:end`) and have `upsert_managed_block` operate scoped to the adapter id. Workaround for this validation pass: tested wrapper adapters sequentially, hand-editing `state.json` to remove the prior wrapper entry between runs.
  - **Bug C — opencode plugin schema rejects Trove's `_trove` marker key.** Trove's opencode adapter writes a `_trove: {…}` block alongside `plugin: […]` to record provenance/customAttributes. The opencode CLI's strict schema (`https://opencode.ai/config.json`) rejects unknown top-level keys with `Unrecognized key: _trove` and refuses to start. The plugin itself works fine without the marker. Fix direction: store Trove provenance in `state.json` only (where the rest of the per-adapter metadata already lives) rather than echoing it into the consumer config, or wrap it under a known-extension key the schema tolerates. Workaround for this validation pass: deleted `_trove` from `~/.config/opencode/opencode.json` by hand; plugin still emits telemetry.
  - **Bug D — `copilot-cli` adapter targets the deprecated `gh-copilot` extension.** Trove's wrapper installs a `gh-copilot()` shell function, but GitHub deprecated `gh-copilot` on 2025-09-25 in favor of the standalone `github/copilot-cli` (binary name `copilot`). The wrapper still fires for users with the legacy extension installed (Jeff's machine still has it, hence the PASS this run), but the adapter will quietly capture nothing on a clean install of the new CLI. Fix direction: rename the adapter to wrap `copilot` (or both, since the deprecated CLI is still available for ~6 more months) and update the harness id/name accordingly.
- **Sensitive material flagged for rotation (not echoed/committed):**
  - `ANTHROPIC_API_KEY` in `~/.zshrc:12` — exposed during the autonomous run via Read.
  - `DASHSCOPE_API_KEY` in `~/.qwen/settings.json` — exposed during qwen-code config inspection.
- **Notes / follow-up:**
  - The Tier-3 wrappers' service.name resolution still relies on the adapter setting it directly when crafting the OTLP LogRecord; the codegen.rs `transform/harness-tag` block does not yet enumerate `aider` / `copilot-cli`. Cosmetic at query time (queries by `harness.id` work) but worth adding for parity before release. Same gap as the cursor-cli §/¶ run.
  - Both wrapper smokes used `zsh -i -c "…"` so the shell function definition is sourced from the rc; running the bare command in this Tauri/Claude subshell would have hit the real binary instead of the wrapper.
  - Telemetry-only PASS — the in-app enable/disable flow for these four adapters is **not** PASS until Bugs A–D are addressed. Each of the four bugs is independently small (1 file, ~10–40 LOC) and should be in scope for the release-blockers commit train before tagging.

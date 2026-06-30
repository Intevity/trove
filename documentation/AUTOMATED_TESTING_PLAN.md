# Trove harness × platform end-to-end test plan

> Companion doc to [`RELEASE_CHECKLIST.md`](RELEASE_CHECKLIST.md). The results
> matrix lives at [`harness-platform-matrix.md`](harness-platform-matrix.md).

## Context

Trove (pre-release branch) ships 16 harness adapters and 15 platform presets.
Before release we need empirical proof that **every harness we can exercise**
lands real telemetry in **every platform preset** — both the OSS targets
(which we run locally) and the cloud targets (where we have credentials). The
existing `/Users/jeff/github/observability/grafana-local` stack covers
Grafana; this plan stands up peer stacks for the remaining OSS targets and
codifies a per-pairing verification protocol (collector receipt **and**
read-side query) so we end up with a results matrix that gates release.

Telemetry topology recap:

```
Harness (claude, agy, ...)
   │  OTLP HTTP/protobuf
   ▼
Trove's bundled collector (127.0.0.1:4318)   ← driven by the active preset
   │  per-preset exporter
   ▼
Platform OTLP intake (local docker on 1431x, or cloud OTLP endpoint)
```

Trove is a Tauri GUI — there is no headless IPC to toggle harness adapters,
so **the operator drives every harness enable/disable in the Trove UI**;
Claude drives everything downstream of that (running the harness CLI,
querying platforms, reading collector logs).

---

## Section 1 — Harness test matrix

Source of truth: `packages/app/src-tauri/src/adapters/*.rs`, cross-checked
against installed binaries on the dev machine.

### 1a. Claude-drivable today (installed CLIs)

| Harness id        | Binary                      | Smoke command Claude can run              |
| ----------------- | --------------------------- | ----------------------------------------- |
| `claude-code`     | `~/.local/bin/claude`       | `claude -p "say hi" --output-format text` |
| `antigravity-cli` | `/opt/homebrew/bin/agy`     | `agy -p "say hi"`                         |
| `cursor-cli`      | `~/.local/bin/cursor-agent` | `cursor-agent -p "say hi"`                |

Per-test loop: operator toggles the harness ON in Trove for the active preset
→ Claude runs the smoke command → Claude verifies receipt + read-side query
(see §4).

### 1b. Operator-driven (GUI apps or VS Code extensions)

| Harness id       | Why operator                                                                                                                |
| ---------------- | --------------------------------------------------------------------------------------------------------------------------- |
| `claude-desktop` | macOS GUI; Trove taps Cowork's `local-agent-mode-sessions/*/audit.jsonl`. Operator must open Claude Desktop and run a turn. |
| `cursor-ide`     | GUI; uses the same `~/.cursor/hooks.json` patch as `cursor-cli` but invoked by the editor UI. Operator drives Cursor IDE.   |
| `cline`          | VS Code extension; Trove watches `globalStorage/saoudrizwan.claude-dev/tasks`. Operator runs a Cline task in VS Code.       |

### 1c. Needs install before either of us can test

These adapters exist in Trove but the CLI is not on PATH. Operator confirms
install/skip per harness before we exercise them.

| Harness id                                                  | Install hint                                                                                      |
| ----------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| `codex-cli`                                                 | `npm i -g @openai/codex` (needs OpenAI API key)                                                   |
| `qwen-code`                                                 | `npm i -g @qwen-code/qwen-code`                                                                   |
| `opencode`                                                  | `npm i -g opencode-ai`                                                                            |
| `aider`                                                     | `pipx install aider-chat` (wraps `~/.zshrc`)                                                      |
| `copilot-cli`                                               | `gh extension install github/gh-copilot` (wraps `~/.zshrc`)                                       |
| `junie-cli`, `droid`, `kimi-code-cli`, `devin`, `forgecode` | Detection-only in Trove today (`packages/app/src-tauri/src/harness.rs`) — confirm scope per pass. |

---

## Section 2 — OSS platforms to stand up locally

All under `/Users/jeff/github/observability/<id>-local/`, peer to the
existing `grafana-local/`. Each stack is `docker compose up`, test, `docker
compose down -v`; we never run two simultaneously. Distinct host OTLP ports
per stack so swapping presets in Trove never collides with a half-stopped
stack.

| Trove preset  | New dir             | Image set                                                                                                                                                                                                         | UI port               | OTLP gRPC                   | OTLP HTTP                   | Notes                                                                                         |
| ------------- | ------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------- | --------------------------- | --------------------------- | --------------------------------------------------------------------------------------------- |
| `signoz`      | `signoz-local`      | Thin vendor-wrapper around `SigNoz/signoz@v0.124.0`. Inline compose hit ClickHouse cluster XML + OpAMP wiring drift; vendoring tracks upstream cleanly. Run `./scripts/install.sh` once before `./scripts/up.sh`. | http://localhost:8080 | `127.0.0.1:4317` (upstream) | `127.0.0.1:4318` (upstream) | OTLP receivers bind only after admin signup creates an org.                                   |
| `openobserve` | `openobserve-local` | `public.ecr.aws/zinclabs/openobserve:latest` single binary                                                                                                                                                        | http://localhost:5080 | `127.0.0.1:14321`           | `127.0.0.1:14322`           | Lightest. First target.                                                                       |
| `clickstack`  | `hyperdx-local`     | HyperDX OSS all-in-one (`hyperdx/hyperdx-all-in-one`)                                                                                                                                                             | http://localhost:8080 | `127.0.0.1:14323`           | `127.0.0.1:14324`           | ClickHouse + OTel collector bundled.                                                          |
| `opensearch`  | `opensearch-local`  | `opensearchproject/opensearch` + `opensearch-dashboards` + `opensearchproject/data-prepper`                                                                                                                       | http://localhost:5601 | `127.0.0.1:14325`           | `127.0.0.1:14326`           | Data Prepper is the OTLP receiver; matches `templates/opensearch.yaml`.                       |
| `elastic`     | `elastic-local`     | `elasticsearch:8.x` + `kibana:8.x` + `apm-server:8.x` with OTLP intake enabled                                                                                                                                    | http://localhost:5602 | `127.0.0.1:14327`           | `127.0.0.1:14328`           | Heavy (~4 GB RAM). Kibana port shifted to avoid collision with OpenSearch Dashboards on 5601. |
| `sentry`      | `sentry-local`      | Sentry's official `self-hosted` repo (`install.sh`)                                                                                                                                                               | http://localhost:9000 | n/a                         | `127.0.0.1:14330`           | Heaviest. OTLP traces ingest is HTTP only. Schedule last.                                     |

Each new dir gets the same shape as `grafana-local/`: `docker-compose.yml`,
`README.md`, `.env.example`, optional `otel-collector/config.yaml` if the
platform needs one in front of it, and a `scripts/smoke.sh` that emits a
synthetic OTLP span/metric/log so the stack can be sanity-checked without
any harness.

**Out of scope locally** (no usable OSS edition): `dynatrace`, `datadog`,
`honeycomb`, `grafana-cloud`, `new-relic`, `splunk-observability`,
`chronosphere`, `clickstack`-SaaS, `sentry`-SaaS — all covered in §3.

---

## Section 3 — Cloud platforms Claude can test with credentials

For each, Claude needs the named secret(s) injected into the Trove preset
(operator pastes them into the Trove UI — the secret never enters the
conversation transcript) plus, where applicable, a **read** API key for
verification queries.

| Trove preset                 | Write creds (preset side)                                                 | Read creds (verify side)                      | Verify call Claude will use                                |
| ---------------------------- | ------------------------------------------------------------------------- | --------------------------------------------- | ---------------------------------------------------------- |
| `signoz` (cloud)             | Ingestion key + region                                                    | Query API token                               | `POST {region}.signoz.cloud/api/v1/query_range`            |
| `honeycomb`                  | `x-honeycomb-team` API key                                                | Same key                                      | `GET api.honeycomb.io/1/queries/{dataset}`                 |
| `grafana-cloud`              | Instance ID + API token (Basic)                                           | Same token, "MetricsPublisher" + "Logs" scope | PromQL via `prometheus-{stack}.grafana.net/api/v1/query`   |
| `datadog`                    | `DD-API-KEY`                                                              | App key for `/api/v1/query`                   | `GET api.datadoghq.com/api/v1/query?query=...`             |
| `new-relic`                  | License key                                                               | User API key for NerdGraph                    | NRQL via `api.newrelic.com/graphql`                        |
| `splunk-observability`       | `X-SF-Token`                                                              | Same                                          | `GET api.{realm}.signalfx.com/v2/signalflow`               |
| `dynatrace`                  | API token w/ `openTelemetryTrace.ingest`, `metrics.ingest`, `logs.ingest` | Token w/ `metrics.read`, `entities.read`      | Metrics v2 query API                                       |
| `elastic` (cloud)            | APM-server API key                                                        | ES query API key                              | `_search` on `traces-apm-*`, `logs-apm-*`, `metrics-apm-*` |
| `clickstack` (HyperDX cloud) | Ingestion key                                                             | Personal API key                              | HyperDX search API                                         |
| `chronosphere`               | API token                                                                 | Same                                          | PromQL via `{tenant}.chronosphere.io/api/v1/query`         |
| `sentry` (SaaS)              | DSN-derived OTLP token                                                    | Auth token w/ `event:read`                    | `GET sentry.io/api/0/projects/{org}/{proj}/events/`        |

When operator is ready to test a cloud target, operator tells Claude which
preset → Claude prints exactly which secret types it needs → operator pastes
them into Trove and confirms — Claude does **not** receive the secret
material, only confirmation that the preset is active.

---

## Section 4 — Verification protocol (per harness × platform pairing)

For each cell:

1. **Collector receipt.** Read Trove's bundled collector log (path in the
   Trove UI under Diagnostics → Collector logs) and confirm a recent entry
   of the form `accepted N spans|metrics|logs` whose resource attributes
   include `harness.id=<id>` and `harness.name=<display>` (pinned in adapter
   source, e.g. `claude_code.rs:74-77`). Record: timestamp, signal counts,
   harness.id observed.
2. **Read-side query.** Issue a query against the platform that filters on
   `harness.id` and a recent time window (last 10 min). Body is
   platform-specific (PromQL / SigNoz query API / OpenObserve search /
   OpenSearch DSL / Sentry events endpoint). Record: query, count returned,
   sample row.
3. **Results matrix.** Append a row to
   [`harness-platform-matrix.md`](harness-platform-matrix.md). Columns:
   harness, platform, date, receipt PASS/FAIL, query PASS/FAIL, notes.
   Cells with receipt-PASS / query-FAIL usually indicate a schema mapping
   bug in `packages/collector-presets/templates/<platform>.yaml`.

---

## Section 5 — Suggested execution order

Local stacks, easiest → heaviest:

1. `openobserve-local`
2. `hyperdx-local`
3. `signoz-local`
4. `opensearch-local`
5. `elastic-local`
6. `sentry-local`

Within each stack: drive `claude-code` first, then `antigravity-cli`, then
`cursor-cli`, then operator drives `claude-desktop`, `cursor-ide`, `cline`.
Cloud platforms run in any order once credentials are in hand.

---

## Section 6 — Execution checklist

### Phase A — Doc & skeleton (Claude, no operator needed)

- [x] Write this plan to `documentation/AUTOMATED_TESTING_PLAN.md`.
- [x] Create `documentation/harness-platform-matrix.md` with the empty
      results matrix.

### Phase B — Build local platform stacks (Claude, no operator needed)

For each stack, ship: `docker-compose.yml`, `README.md`, `.env.example`,
optional `otel-collector/config.yaml`, and `scripts/smoke.sh` emitting a
synthetic OTLP signal.

- [x] `observability/openobserve-local/`
- [x] `observability/hyperdx-local/`
- [x] `observability/signoz-local/` _(switched to vendor wrapper; see §2)_
- [x] `observability/opensearch-local/`
- [x] `observability/elastic-local/`
- [x] `observability/sentry-local/` _(vendor wrapper for getsentry/self-hosted)_

### Phase C — Stack-only smoke tests (Claude, no operator needed)

For each stack: `docker compose up -d` → wait for healthy → run
`scripts/smoke.sh` → confirm the synthetic signal lands in the platform's
UI/API → `docker compose down -v`.

- [x] openobserve-local smoke — **PASS**
- [x] hyperdx-local smoke — **PASS** _(2026-05-22: promoted from PARTIAL after operator completed onboarding + ingestion key paste; verified end-to-end via the 2026-05-22 `◆` run-log entry in the matrix)_
- [x] signoz-local smoke — **PASS** _(2026-05-22: promoted from PARTIAL via the same `◆` run; ClickHouse-direct queries confirm logs+spans for every queryable harness)_
- [x] opensearch-local smoke — **PASS** (needs ~3 min wait for Data Prepper's Trace Raw Processor flush)
- [x] elastic-local smoke — **PASS** _(2026-05-22: brought up via `docker compose up -d`; APM Server 8.16 accepts OTLP HTTP on `:14328`; per-service `.ds-{logs,metrics,traces}-apm.app.<svc>-*` indices verified)_
- [x] sentry-local smoke — **PASS** _(2026-05-25: arm64 install fixed by bumping vendor to v26.5.0 + cp-kafka 7.6.6→7.9.0 + adding `CONTROLLER://127.0.0.1:29093` to `KAFKA_ADVERTISED_LISTENERS` + `KAFKA_HEAP_OPTS=-Xmx2G`. Trove preset endpoint corrected envelope→`/api/<id>/integration/otlp/`. See matrix.md footnote `★`)_

### Phase D — Harness × local-platform pairings

Operator toggles harness in Trove; Claude runs harness + verifies. One row
in [`harness-platform-matrix.md`](harness-platform-matrix.md) per cell.

|                     | grafana-local    | openobserve-local | hyperdx-local    | signoz-local | opensearch-local | elastic-local    | sentry-local     |
| ------------------- | ---------------- | ----------------- | ---------------- | ------------ | ---------------- | ---------------- | ---------------- |
| claude-code         | [x]              | [x]               | [x]              | [x]          | [x]              | [x]              | [x]              |
| antigravity-cli     | [x]              | [x]               | [x]              | [x]          | [x]              | [x]              | [x]              |
| cursor-cli          | [x]              | [x]               | [x]              | [x]          | [x]              | [x]              | [x]              |
| claude-desktop (op) | [x]              | [x]               | [x]              | [x]          | [x]              | [x]              | [ ] _(⊕)_        |
| cursor-ide (op)     | [x]              | [x]               | [x]              | [x]          | [x]              | [x]              | [x]              |
| cline (op)          | [x] _(◑)_        | [x] _(◔)_         | [x] _(◑)_        | [x] _(◔)_    | [x] _(◑)_        | [x] _(◑)_        | [x] _(◑)_        |
| codex-cli           | [x]              | [x]               | [x]              | [x]          | [x] _(☆ spans)_  | [x]              | [x]              |
| qwen-code           | [x]              | [x]               | [x]              | [x]          | [x]              | [x]              | [x]              |
| opencode            | [x] _(◑ Q:FAIL)_ | [x]               | [x] _(◑ Q:FAIL)_ | [x]          | [x] _(◑ Q:FAIL)_ | [x] _(◑ Q:FAIL)_ | [x] _(◑ Q:FAIL)_ |
| aider               | [x]              | [x]               | [x]              | [x]          | [x]              | [x]              | [x]              |
| copilot-cli         | [x]              | [x]               | [x]              | [x]          | [x]              | [x]              | [x]              |

### Phase E — Cloud platform pairings

One row per cloud platform per installed CLI harness. Skip cells where
operator doesn't have creds.

- [ ] signoz cloud
- [ ] honeycomb
- [ ] grafana-cloud
- [ ] datadog
- [ ] new-relic
- [ ] splunk-observability
- [ ] dynatrace
- [ ] elastic cloud
- [ ] clickstack (HyperDX cloud)
- [ ] chronosphere
- [ ] sentry SaaS

---

## Section 7 — This-session scope

**Will finish this session, no blocking on operator:**

- All of Phase A.
- Phase B for **all six** stacks (heavy ones still get a compose file; only
  the smoke run is deferred).
- Phase C for the four light stacks: openobserve, hyperdx, signoz,
  opensearch.

**Will attempt this session, but each pairing blocks on operator toggling
the harness in Trove and confirming:**

- Phase D for the **three installed CLIs** (`claude-code`, `antigravity-cli`,
  `cursor-cli`) × the four light stacks plus existing grafana-local. ~15
  cells.

**Explicitly deferred to a follow-up:**

- Elastic and Sentry self-hosted smoke + harness pairings (heavy stacks;
  compose dirs ready, no boot).
- All operator-driven GUI harnesses (Claude Desktop, Cursor IDE, Cline) —
  pending a dedicated GUI-test session.
- All `*(install)*` harnesses in §1c — pending operator install decision per
  harness.
- Phase E cloud pairings — pending operator providing creds to Trove for
  each preset.

---

## Critical files

- `packages/app/src-tauri/src/adapters/*.rs` — harness adapter specs.
- `packages/app/src-tauri/src/harness.rs` — `HarnessId` enum incl.
  detection-only ids.
- `packages/collector-presets/templates/*.yaml` — preset collector configs
  (where mapping bugs would live).
- `packages/shared/src/schemas.ts` — `Backend` union and preset metadata.
- `packages/shared/src/constants.ts` — `PRESETS` array.
- `/Users/jeff/github/observability/grafana-local/docker-compose.yml` —
  reference layout for new stacks.
- New: `/Users/jeff/github/observability/{openobserve,signoz,hyperdx,opensearch,elastic,sentry}-local/`.
- New: `documentation/harness-platform-matrix.md`.

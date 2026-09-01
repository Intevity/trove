# Trove Roadmap

Where Trove goes over roughly the next two years, and — just as importantly — what it
will not become.

This document is a companion to [`MVP_PLAN.md`](MVP_PLAN.md), which is a historical
record of the 17 sprints that shipped Trove 0.x and is no longer a planning artifact.
Everything forward-looking lives here.

## Context

Trove 0.8.6 does one thing well: it finds the AI coding tools on your machine, patches
each one's telemetry config to emit OTLP at a local collector, normalizes the
cross-vendor mess onto a common schema, and forwards it to a backend you already own.
Eighteen harness identifiers are registered, thirteen have working adapters, fifteen
platform presets ship, and the whole thing is a reversible, localhost-only, MIT-licensed
tray app with no SaaS layer and no telemetry of its own.

Three things about the current moment shape everything below.

**The measurement problem got expensive.** Gartner projects AI coding costs will surpass
the average developer's salary by 2028. Uber rolled Claude Code out to ~5,000 engineers
in December 2025 and had burned its entire 2026 AI budget by April. Harness's 2026
survey found 94% of engineering leaders say the metrics that matter most are missing
from their frameworks. The demand Trove was built for is arriving faster than the
product is maturing.

**Trove is not the only one who noticed.** Dynatrace ships coding-agent monitoring for
Claude Code, Gemini CLI, Codex CLI, OpenCode and Copilot. The OSS `opentelemetry-hooks`
project does hook-based capture across eight agents. Trove's advantages — breadth,
byte-for-byte reversible patching, local-first, no vendor in the dependency tree — are
real but not permanent.

**The gap between what Trove claims and what Trove has proven is the biggest risk to its
credibility.** The [harness × platform matrix](harness-platform-matrix.md) shows seven
local Docker stacks essentially fully passing. It also shows that **all eleven cloud and
SaaS platform columns are empty — never validated once** — and only six of those carry a
Beta pill in the UI. Fixing that comes before anything else on this list.

## How to read this

Items sit in one of three horizons. There are no dates, because contributor capacity is
the binding constraint and dated commitments in a public repo mostly serve to be missed.

- **Now** — active or next up. Roughly the 0.9 → 1.0 arc.
- **Next** — committed in principle, sequenced behind Now. Roughly 1.x.
- **Later** — directionally agreed, not designed. 2.0 and beyond.

Each item carries a **problem** (why it exists at all), what ships, a rough **size**
(S / M / L), and its dependencies. Items are promoted when their dependencies land and
someone picks them up — not on a calendar.

Four durable **themes** run through all three horizons:

| Theme     | The question it answers                                         |
| --------- | --------------------------------------------------------------- |
| **Trust** | Can you believe what Trove tells you, and what it tells others? |
| **Reach** | How many of the tools you actually use does it cover?           |
| **Depth** | Is Trove worth running on its own merits?                       |
| **Scale** | Does it work for 200 engineers, not just one?                   |

## Principles

These are constraints on the roadmap, not aspirations. An item that violates one does
not get built.

1. **Local-first, and MIT.** Every artifact Trove ships runs on hardware the user
   controls.
2. **The open-core boundary is: we build it, you run it.** Fleet aggregation and the
   Relay both ship as self-hostable artifacts the customer deploys. Intevity operates no
   hosted tier. "Your data goes to your backend, never ours" stays literally true — not
   softened later into "local by default."
3. **No Trove-side telemetry.** No analytics, no crash reporting, no phoning home. If a
   future feature needs aggregate data, it is opt-in, anonymized, and it is a decision
   made in the open — see [Open decisions](#open-decisions).
4. **Every patch stays reversible.** Sentinel-bracketed, atomic, byte-for-byte
   revertible, refuses to clobber hand edits. This is a load-bearing property, not a
   nicety.
5. **Integrate with eval and LLM-observability platforms; do not become one.** Trove
   moves signals. Other people's products score them.

---

## Now

The credibility floor. Trove currently ships presets it has never proven and documents
claims that are no longer true. Nothing on the Next list matters if a user's first
experience is a backend that silently drops their telemetry.

### N1 — Fill the cloud validation matrix · Trust · L

**Problem:** Eleven of the fifteen platform presets have never been validated end to
end. A user configuring Datadog, New Relic or Grafana Cloud today is the first person to
try it.

Run one focused campaign against free and trial tiers of all eleven cloud/SaaS columns,
using the receipt-plus-query protocol in
[`AUTOMATED_TESTING_PLAN.md`](AUTOMATED_TESTING_PLAN.md) §4: confirm the collector
accepted a batch tagged `harness.id`, then confirm the platform's own read API returns
the row. Record every cell in the matrix with its dated run-log entry. Fix what breaks.

_A nightly automated sweep is the durable answer, but it needs long-lived vendor accounts
that expire and churn. Manual first; automate once the shape is known._

### N2 — Make the Beta pill tell the truth · Trust · M

**Problem:** `PresetMetadata.beta` is hand-maintained and has drifted from reality. Six
presets carry it. Meanwhile `elastic`, `opensearch`, `openobserve`, `clickstack`,
`sentry` and `grafana-cloud` carry no warning at all despite having entirely empty cloud
columns — they were validated only against local Docker stacks.

Derive the flag from matrix state rather than maintaining it by hand, and split the
signal in two: **local-validated** and **cloud-validated** are different claims and the
UI should stop conflating them. Same treatment for `HARNESS_BETA` on the harness side.

**Depends on:** N1.

### N3 — Stale-artifact cleanup · Trust · S

**Problem:** Several documented facts are no longer facts, and one linked document does
not exist.

| Artifact                                                        | Fix                                                                                                            |
| --------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `documentation/adding-a-backend.md`                             | **Write it.** Linked by `.github/ISSUE_TEMPLATE/backend-request.yml` and `CONTRIBUTING.md`; has never existed. |
| [`harness-platform-matrix.md`](harness-platform-matrix.md)      | Retire the `gemini-cli` row — that identifier is in neither enum any more.                                     |
| `README.md`                                                     | "pre-1.0 builds ship unsigned" is wrong; macOS notarization and Windows Trusted Signing both shipped.          |
| `packages/site/.../getting-started/introduction.mdx`            | Still says "currently at version 0.5.0".                                                                       |
| [`AUTOMATED_TESTING_PLAN.md`](AUTOMATED_TESTING_PLAN.md)        | Says "16 harness adapters".                                                                                    |
| `harness.rs`, `detect/paths.rs`, `mappings/defaults.rs`, README | All describe Droid as detection-only. It is in `tier_1()` with a full adapter and watcher.                     |

### N4 — Fix the OpenCode span timestamps · Trust · S

**Problem:** OpenCode is a recurring `Q:FAIL` on five of seven local stacks. Root cause
is a skewed span start-timestamp in `@devtheops/opencode-plugin-otel`: stores that index
traces by ingest time (SigNoz, OpenObserve) find the spans; stores that index by span
start time (Tempo, Elastic APM, Sentry) ingest them and then cannot search them.

Fix upstream, or vendor a patched build until upstream lands it.

### N5 — Headless `trovectl` · Reach · L

**Problem:** Trove's entire value proposition assumes a desktop with a tray icon. CI
runners, devcontainers and cloud agent sandboxes have none of that, and that is where a
growing share of agent turns now happen.

Ship the collector, adapters and supervisor as a GUI-less binary configured from a file
and environment variables. Same Rust core, same reversible-patch contract, no WebView.
This is the prerequisite for every cloud-agent item on the Next list.

### N6 — Local telemetry store · Depth · L

**Problem:** Trove persists nothing but configuration. A new user gets zero value until
they have stood up an observability backend, which is a large ask for a solo developer
and a slow one for a team.

Add an embedded store with a **90-day default retention** and a disk budget. DuckDB is
the leading candidate over SQLite: span and metric browsing is overwhelmingly
column-oriented scan-and-aggregate work.

**This reverses a deliberate architectural decision.** Today's "no database" stance is
part of the security story, and abandoning it is a real trade, made because the
activation cost of a forwarder-only product is too high. It forces a threat-model
rewrite — see [Open decisions](#open-decisions).

### N7 — Built-in observability UI · Depth · L

**Problem:** Even with a store, Trove has nowhere to show the data. The app has six tabs
and none of them display a span.

A metrics, spans and logs browser with a date picker and filters by harness, model and
session. Not a Grafana competitor — enough to answer "what did my agents do this week,
and what did it cost" without configuring anything. Fan-out to a real backend stays the
path for teams and long retention.

Together with N6 this is the single biggest activation unlock on the roadmap: it makes
Trove useful on first launch with zero backends configured.

**Depends on:** N6.

### N8 — Dashboards-as-code · Scale · M

**Problem:** [`documentation/dashboards/`](dashboards/) ships sample dashboards for three
of fifteen platforms. For the other twelve, connecting a backend ends with an empty
datasource and a homework assignment.

Author the five shared Tier-A panels for all fifteen, and push them through each vendor's
provisioning API directly from the setup wizard, so "connect backend" terminates in
working dashboards.

---

## Next

From pipe to platform. These turn Trove from something that moves telemetry into
something that answers questions.

### X1 — Trove SDK · Reach · M

**Problem:** A team that builds a bespoke harness on the Claude Agent SDK has no way in.
There is no config file for Trove to patch, and no tray app in their container.

An npm and PyPI package that emits the Tier-A schema directly. Documentation must call
out the TypeScript footgun explicitly: `options.env` **replaces** the inherited
environment rather than merging into it, so callers have to spread `process.env` or
their exporter configuration silently vanishes. (Python's `env` merges; the asymmetry
bites people.)

**Depends on:** N5.

### X2 — Cloud and background agents · Reach · L

**Problem:** Claude Code Remote Tasks and Routines, Codex cloud, the GitHub Copilot
coding agent, Jules, Cursor cloud agents and Devin are the fastest-growing slice of the
market, and Trove's config-patching architecture cannot reach a single one of them. There
is no local file to patch.

Reached instead through `trovectl` running as a sidecar, the SDK linked into the harness,
or a self-hosted Relay for sandboxes that allow neither. Per-provider recipes for GitHub
Actions and GitLab CI ship alongside.

**Depends on:** N5, X1, L2.

### X3 — OSS terminal agents · Reach · M

**Problem:** Goose, Sourcegraph Amp, Crush, Continue, Kilo Code, OpenHands, Plandex and
Pi are all unsupported, and several are growing fast.

Adapters for each. These are the cheapest wins on the Reach list and, being open source,
the most likely to accept an upstream OTel patch instead of needing a shell wrapper —
which is what L4 is for.

### X4 — Enterprise and IDE agents · Reach · L

**Problem:** The highest-value-per-seat tools are the least covered. Kiro CLI — the
closed-source successor to the Amazon Q Developer CLI — Windsurf, Zed's agent and Warp
have no adapters at all.

Add them, and promote the four detection-only identifiers (`junie-cli`, `kimi-code-cli`,
`devin`, `forgecode`) from a row in the UI to a working adapter. Several are closed
source with no hook surface, so expect Tier-3 wrappers and log watchers rather than clean
config patches.

### X5 — Eval and LLM-observability presets · Reach · L

**Problem:** Langfuse, Braintrust, Arize Phoenix, LangSmith, Logfire and Laminar are
where AI engineers already look, and Trove sends them nothing.

These are **not** another header preset. They expect GenAI and OpenInference semantic
conventions, so this needs a genuine translation processor mapping Trove's Tier-A schema
onto `gen_ai.*` attributes. Note the conventions' own caveat: core chat and embedding
attributes are stable, but agent and tool-orchestration conventions are still settling
and should be treated as provisional.

### X6 — Git outcome attribution · Depth · L

**Problem:** Token counts are not an outcome. Gartner is explicit that tokens consumed
have no direct relationship to productivity, and METR found developers were 19% slower
with AI tools while believing they were 20% faster. A dashboard of token spend cannot
tell you whether any of it worked.

Tag telemetry with branch, commit and PR; follow those through to merged, reverted or
abandoned; surface **cost per merged PR**. This is the metric every engineering leader is
asking for and nobody currently ships.

**Depends on:** N6.

### X7 — Budgets, alerts and runaway guardrails · Depth · M

**Problem:** Trove watches an agent burn a month of budget in an afternoon and says
nothing. It is a pipe.

Per-day and per-week spend budgets, detection for token spikes, tool-call loops and retry
storms, desktop notification, and an optional kill switch. Turns Trove into a control
plane and targets the failure mode that took out Uber's AI budget.

**Depends on:** N6.

### X8 — Security and SIEM lane · Trust · L

**Problem:** Trove pitches a security and IT persona and ships them nothing specific.
Meanwhile Claude Code already emits exactly the events a security team wants —
`tool_decision`, `permission_mode_changed`, `mcp_server_connection` — and Trove treats
them as undifferentiated log records.

Route security-relevant events as a first-class signal, with presets for Splunk
Enterprise Security, Microsoft Sentinel and Elastic SIEM, plus a rules-driven redaction
processor to replace today's fixed `attributes/redact` (which deletes exactly
`user.prompt` and `prompt.text` and nothing else) and configurable retention.

**Depends on:** X9.

### X9 — Per-signal routing · Trust · M

**Problem:** Fan-out is all-or-nothing. Every signal goes to every enabled platform,
which is fine for two dashboards and wrong the moment one destination is a SIEM.

Per-signal, per-destination routing, so security events go to the SIEM, traces to the
tracing backend and metrics to the time-series store — the topology OpenTelemetry's own
guidance recommends and Trove currently cannot express.

### X10 — Adapter and preset plugin SDK · Scale · L

**Problem:** Eighteen harnesses and fifteen presets, both growing monthly, and every
single addition requires a Trove release. The registry is the bottleneck.

A signed-manifest plugin format so third parties ship adapters and presets out of band.
Signing is non-negotiable: an adapter writes to config files in the user's home
directory.

**Depends on:** L6.

---

## Later

Organizational scale. Directionally agreed; not designed.

### L1 — Fleet mode · Scale · L

**Problem:** The README pitches dead-seat detection across 200 engineers and cross-vendor
cost normalization for procurement. Neither is buyable, because there is no way to deploy
Trove to 200 machines with policy or to see across them.

MDM- and GPO-pushable managed configuration, plus an aggregator the **customer runs** —
never one Intevity operates. Per principle 2, this ships as a container and a Terraform
module, not a login page.

### L2 — Self-hosted Relay · Scale · M

**Problem:** Some cloud agent sandboxes are ephemeral and locked down enough that there
is nowhere to run a collector at all.

An OTLP ingest endpoint the customer deploys in their own infrastructure, which cloud
agents post to and which forwards onward. Same architecture as the local collector,
different deployment target. Intevity operates no instance of it.

### L3 — MCP servers and agent frameworks · Reach · L

**Problem:** Coding CLIs are one slice of a much bigger surface. MCP servers, LangGraph,
CrewAI, Mastra, Pydantic AI and bespoke Agent SDK applications all have the same
telemetry problem and no equivalent of Trove.

Repositions Trove from "coding-agent telemetry" to "the local OpenTelemetry front door
for every AI agent." Much larger addressable surface; correspondingly fuzzier focus,
which is why it is here and not in Next.

### L4 — Upstream the conventions · Reach · M

**Problem:** Every Tier-3 shell wrapper and log watcher is permanent maintenance debt
that breaks whenever upstream changes its log format.

Rather than only writing adapters, contribute native GenAI-convention OTel emission to
the open-source agents themselves. Slow, and it hands some differentiation to competitors
— but it shrinks Trove's maintenance surface permanently and is the right thing for the
ecosystem.

### L5 — Trove Index · Depth · M

**Problem:** Nobody can answer whether Claude Code or Codex is cheaper per accepted
change for their team, on their codebase.

Local cross-harness comparison — cost per turn, error rate, turn duration by harness and
model — and, separately, an opt-in anonymized public benchmark report.

**The opt-in upload sits in genuine tension with "never phones home, period."** That is
flagged as an open decision below rather than quietly resolved here.

### L6 — Supply-chain hardening · Trust · M

**Problem:** Trove is a signed desktop app that writes to config files in your home
directory, and X10 proposes letting third parties extend it.

SBOM generation, reproducible builds, and a signed adapter-manifest chain. Prerequisite
for the plugin SDK at any scale.

---

## Explicitly not doing

Naming these keeps them from being relitigated in every issue thread.

- **An Intevity-operated SaaS.** Not fleet aggregation, not a Relay, not "just" a
  dashboard. See principle 2.
- **Telemetry about Trove itself.** No analytics, no crash reporting, no usage pings.
- **Per-vendor native exporter components.** Generic OTLP plus headers keeps the ocb
  manifest slim and the binary small. The Datadog exporter is compiled in and
  deliberately unused; that stays true.
- **Becoming an eval or prompt-management platform.** X5 integrates with them.
- **Agent orchestration.** Trove observes agents. It does not run them.

## Open decisions

Unresolved. Each blocks or reshapes items above.

**What 1.0 means.** Proposal: the cloud matrix is filled (N1), the Beta flags are honest
(N2), every claim in the README is true (N3), and `trovectl` has shipped (N5). Version
1.0 should mean "the documentation is accurate," not "we added features."

**The `SECURITY.md` rewrite that N6 and N7 force.** Today's threat model rests on Trove
persisting nothing but configuration. A local store may hold prompt and tool content on
disk for 90 days. That changes the model, the disclosure, and probably the default —
possibly structural-only capture unless the user opts into content. This is not optional
and it lands with N6, not after it.

**Retention defaults versus disk budget.** Ninety days of spans for a heavy
multi-harness user is not small. What is the cap, and what gets dropped first when it is
hit — oldest first, or content before structure?

**How the Fleet aggregator authenticates** without introducing the account system
principle 2 rules out. mTLS, an org-wide pre-shared key, and existing SSO in front of a
self-hosted deployment are all candidates.

**Whether L5's opt-in upload can coexist with "never phones home, period."** This is a
copy and positioning decision, not an engineering one, and it needs answering before L5
is designed rather than after.

---

## Appendix — current-state snapshot

Accurate as of 0.8.6. This section is expected to drift; the matrix and the source are
authoritative.

**Harnesses** — 18 identifiers registered in `packages/app/src-tauri/src/harness.rs` and
`packages/shared/src/schemas.ts`.

| Tier                       | Harnesses                                                                           |
| -------------------------- | ----------------------------------------------------------------------------------- |
| **1** — native OTel        | `claude-code`, `claude-desktop`, `droid`, `codex-cli`, `codex-desktop`, `qwen-code` |
| **2** — Trove-shipped hook | `cursor-ide`, `cursor-cli`, `opencode`, `antigravity-cli`                           |
| **3** — best effort        | `cline`, `aider`, `copilot-cli`                                                     |
| Detection only             | `junie-cli`, `kimi-code-cli`, `devin`, `forgecode`, `sentinel`                      |

**Platforms** — 15 presets in `packages/collector-presets/src/index.ts`, all generic OTLP
exporter plus header preset. Six carry a Beta flag (`honeycomb`, `datadog`, `new-relic`,
`splunk-observability`, `dynatrace`, `chronosphere`); see N2 for why that set is wrong.

**Validation** — seven local Docker stacks broadly passing; eleven cloud/SaaS columns
entirely unvalidated. Live state in
[`harness-platform-matrix.md`](harness-platform-matrix.md).

**Architecture** — Tauri 2 tray app (Rust core, React UI) supervising a bundled
`ocb`-built OpenTelemetry Collector on `127.0.0.1:4317`/`:4318`. No database; state is a
single migrated `state.json` (schema v12) and secrets live in the OS keychain. Single
user, no server component. Full tour in [`architecture.md`](architecture.md).

**Tier-A schema** — `trove.harness.events`, `trove.harness.tokens`,
`trove.harness.cost.usd`, `trove.harness.turn.duration`, `trove.harness.errors`. Grammar
and collector semantics in [`MAPPING_PLAN.md`](MAPPING_PLAN.md).

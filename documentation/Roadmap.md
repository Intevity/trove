# Trove Roadmap

What is left for Trove to do, now that Jig exists, and
where the rest of the old roadmap went.

This document is a companion to [`MVP_PLAN.md`](MVP_PLAN.md), which is a historical
record of the 17 sprints that shipped Trove 0.x. The previous version of this roadmap,
written at 0.8.6, planned Trove's growth from a telemetry pipe into a platform. That plan
is superseded; it is in git history for anyone who wants it.

## Context

Trove 0.8.6 does one thing well: it finds the AI coding tools on your machine, patches
each one's telemetry config to emit OTLP at a local collector, normalizes the
cross-vendor mess onto a common schema, and forwards it to a backend you already own.

Most of what the old roadmap wanted Trove to become — a local store, a UI that shows what
your agents did and what it cost, spend guardrails, reach into CI and cloud sandboxes, a
team deployment — was an attempt to reconstruct, from telemetry, a picture of agent work
that something else owns. **Jig owns it.** Jig runs the agents: it drives them over ACP
and, for Claude, the Agent SDK; it keeps every session, its transcript and its spend in
its own store; it runs sessions on a host runner or in per-session pods; it shows
dashboards over those rows; it caps daily spend and bounds goals; and it puts code work
in worktrees that only a person lands, so it knows which session produced which change.
Its tray even runs Trove's supervisor state machine.

What Jig does not do is anything OpenTelemetry. It has no collector, sends nothing to an
existing observability backend, and cannot see an agent a person runs outside it, in
their own terminal or IDE. **Passive observe-and-forward is the one job that is still
Trove's.** This roadmap is sized to that job.

## Principles

Unchanged. These are constraints, not aspirations.

1. **Local-first, and MIT.** Every artifact Trove ships runs on hardware the user
   controls.
2. **We build it, you run it.** Intevity operates no hosted tier. "Your data goes to your
   backend, never ours" stays literally true.
3. **No Trove-side telemetry.** No analytics, no crash reporting, no phoning home.
4. **Every patch stays reversible.** Sentinel-bracketed, atomic, byte-for-byte
   revertible, refuses to clobber hand edits.
5. **Integrate with eval and LLM-observability platforms; do not become one.** Trove
   moves signals. Other people's products score them.

---

## Final release

The credibility floor, and the last feature work planned. Version 1.0 should mean "the
documentation is accurate," not "we added features." Trove currently ships presets it has
never proven and documents claims that are no longer true; that is what this release
fixes.

### N3 — Stale-artifact cleanup · S

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

The README should also say, plainly, that Trove is in maintenance and point at Jig.

### N4 — Fix the OpenCode span timestamps · S

**Problem:** OpenCode is a recurring `Q:FAIL` on five of seven local stacks. Root cause
is a skewed span start-timestamp in `@devtheops/opencode-plugin-otel`: stores that index
traces by ingest time (SigNoz, OpenObserve) find the spans; stores that index by span
start time (Tempo, Elastic APM, Sentry) ingest them and then cannot search them.

Fix upstream, or vendor a patched build until upstream lands it. An upstream fix helps
everyone who uses that plugin, whether or not they use Trove.

### N2 — Make the Beta pill tell the truth · S

**Problem:** `PresetMetadata.beta` has drifted from reality. Six presets carry it, while
`elastic`, `opensearch`, `openobserve`, `clickstack`, `sentry` and `grafana-cloud` carry
no warning despite never having been validated against their cloud offerings.

Hand-flag every preset whose cloud column is empty, and word the pill as "validated
locally only" where that is the truth. The previous plan — deriving the flag from matrix
state, and a split local/cloud signal in the UI — is more machinery than a product in
maintenance needs. Same treatment for `HARNESS_BETA`.

### N1 — Validate the backends people actually use · M

**Problem:** Eleven of the fifteen platform presets have never been validated end to
end.

Validate the two or three that Intevity and Trove's known users actually run, using the
receipt-plus-query protocol in [`AUTOMATED_TESTING_PLAN.md`](AUTOMATED_TESTING_PLAN.md)
§4, and record each cell in the [matrix](harness-platform-matrix.md) with its dated
run-log entry. The rest stay Beta (N2) until someone who uses them reports back. An
eleven-vendor campaign, and the nightly sweep it would need, is not worth running for a
product that is not growing.

**Depends on:** N2, so that whatever is not validated is at least labelled.

## Maintenance policy

After the final release, Trove gets:

- **Security fixes**, on the terms in [`SECURITY.md`](../SECURITY.md).
- **Harness breakage fixes** — when an upstream tool changes its config or log format and
  an existing adapter stops working.
- **Dependency and signing upkeep** needed to keep releases installable.

It does not get new harnesses, new presets, or new features. A new agent belongs in Jig's
agent registry; a request for a new passive adapter is weighed against the open decision
below rather than accepted by default.

---

## Moved to Jig

Good ideas from the old roadmap that are cheaper to build in Jig, because Jig already owns
the session, the spend and the code landing. They are candidates for Jig's own planning,
not commitments.

| Was                                   | In Jig                                                                                                                                                                                                               |
| ------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **X6** — Git outcome attribution      | Cost per landed change. Jig already knows each session's spend and the worktree branch it worked on (decision 0022); joining the two to the PR's outcome needs no telemetry at all.                                  |
| **L5** — Cross-harness comparison     | The local half only: cost, error rate and duration by agent and model, as a dashboard widget (decision 0020). Jig drives several vendors on the same project, which is exactly the comparison L5 wanted.             |
| **X7** — Budgets and guardrails       | Jig has a daily output-token cap and bounded goals. Still missing: loop and retry-storm detection, and cost in dollars rather than output tokens.                                                                    |
| **X5** — GenAI-convention translation | As export: optional OTLP from Jig's sessions in `gen_ai.*` semantic conventions, so a team can send them to Langfuse, Datadog or their own backend. This is where Trove's "your backend, not ours" promise lives on. |
| **X8** — Security and SIEM lane       | The idea, not the design. Jig's run records and boundary refusals are the security signal; a SIEM export of those beats routing raw harness log records.                                                             |
| Principles 1, 3 and 4                 | Local-first, no self-telemetry, reversible patches. Jig's `jig connect` writes hooks into agent config (decision 0016) and should be held to Trove's byte-for-byte reversible-patch contract.                        |

## Retired

Each of these is either already done by Jig or only made sense if Trove was going to
become a platform.

- **N5 — Headless `trovectl`**, **X1 — Trove SDK**, **X2 — Cloud and background agents**,
  **L2 — Self-hosted Relay.** All were ways to reach agent turns off the desktop. Jig runs
  those turns itself, on a host runner or in a session pod, and talks to Claude through
  the Agent SDK directly.
- **N6 — Local telemetry store** and **N7 — Built-in observability UI.** Jig has the store,
  the session history and the dashboards. Building them here would also have forced a
  `SECURITY.md` rewrite for a product being wound down.
- **N8 — Dashboards-as-code.** The three sample dashboards in
  [`dashboards/`](dashboards/) stay; authoring the other twelve does not pay back.
- **X3 — OSS terminal agents** and **X4 — Enterprise and IDE agents.** New agents go into
  Jig's registry. The detection-only identifiers stay detection-only.
- **X9 — Per-signal routing.** Only needed for the SIEM lane, which moved.
- **X10 — Adapter and preset plugin SDK** and **L6 — Supply-chain hardening.** The
  registry is not a bottleneck if it is not growing. Jig has its own extension model and
  release signing.
- **L1 — Fleet mode.** Jig's Helm chart, behind the team's identity provider, is the team
  deployment.
- **L3 — MCP servers and agent frameworks** and **L4 — Upstream the conventions.** Growth
  work for a product that is not growing. L4's one live instance is N4.

---

## Explicitly not doing

- **An Intevity-operated SaaS.** See principle 2.
- **Telemetry about Trove itself.** No analytics, no crash reporting, no usage pings.
- **Per-vendor native exporter components.** Generic OTLP plus headers keeps the ocb
  manifest slim and the binary small.
- **Becoming an eval or prompt-management platform.**
- **Agent orchestration.** Trove observes agents. Running them is Jig's job.

## Open decisions

**Does Jig take on passive observability of agents it did not launch?** This is the one
question that decides Trove's future, and it is Jig's to answer.

- **If not**, Trove stays what this document describes: the final release, then
  maintenance, for as long as people outside Jig find it useful.
- **If so**, Trove's collector and adapters become a Jig component — most naturally beside
  the tray's supervisor, which is already Trove's — and this repository becomes the source
  of that component rather than a product of its own. Nothing under Retired comes back
  either way.

**When Trove is archived.** Once Jig has a public release and either answer above has
landed, set a date, say so in the README, and archive the repository.

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

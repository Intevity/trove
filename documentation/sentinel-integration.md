# Integrating Sentinel with Trove

[Sentinel](https://github.com/Intevity/sentinel) is a Claude Code companion
app that collects Claude Code's OTEL telemetry, enriches it with its own
computed signals, and can forward the combined stream to an external OTLP
endpoint. Trove runs a local OpenTelemetry Collector that receives OTLP and
routes it to your observability backend. The two compose:

```
Claude Code ──env vars set by Sentinel──▶ Sentinel :47284   (collect + enrich)
                                              │ forwarder (OTLP/HTTP JSON)
                                              ▼
                                         Trove :4318         (otelcol receiver, loopback, no auth)
                                              │ codegen'd pipeline
                                              ▼
                                         Backend(s): SigNoz / Honeycomb / Datadog / …
```

The backend sees one stream tagged `service.name=claude-code` (raw Claude Code
telemetry) and `service.name=sentinel` (Sentinel's computed signals).

## Why this isn't a config-patching harness

Trove normally integrates a tool by detecting it and patching its config file
to point its OTEL exporter at Trove's collector. Sentinel is different on two
counts:

1. **Sentinel owns Claude Code.** Sentinel writes the OTEL env vars in
   `~/.claude/settings.json` so Claude Code reports to Sentinel's receiver
   (`127.0.0.1:47284`). Trove's own Claude Code harness writes the _same_ keys
   pointing at `127.0.0.1:4318`. Enabling both makes them overwrite each other.
2. **Sentinel's settings are integrity-signed.** A raw edit by Trove would be
   detected as tampering and ignored (Sentinel falls back to defaults). Only
   Sentinel can validly write its own forwarder configuration.

So in Trove, Sentinel appears as a **detection-only** row: Trove shows that
Sentinel is installed and links to setup instructions, but the wiring happens
inside Sentinel.

## Setup

1. **In Sentinel → Settings → Data → "External OTEL forwarding":** click
   **Forward to Trove**. This sets the OTLP/HTTP endpoint to
   `http://127.0.0.1:4318` and enables metrics + logs forwarding in one step.
   (You can also set the endpoint manually.) No ingestion key is required —
   Trove's receiver is loopback-only, so Sentinel forwards to a local `http://`
   target without auth.
2. **In Trove:** leave the **Claude Code** harness **disabled** (see constraint
   #1 above). Claude Code's telemetry reaches Trove through Sentinel.
3. **In Trove:** add a backend (SigNoz, Honeycomb, Datadog, …). Until a backend
   is configured, Trove receives the data but drops it.

## Verifying

- Sentinel's forwarder status should report `sent > 0`, `failed = 0`.
- Trove's collector counters (loopback Prometheus endpoint on
  `127.0.0.1:18888`) should show received metrics climbing.
- In your backend, filter by `service.name`: `claude-code` for the raw stream
  and `sentinel` for the enriched signals (cache-TTL breakdown, per-account
  usage, account switches, security events, …).

## Notes

- Sentinel forwards to a single endpoint at a time. Routing through Trove lets
  you fan out to multiple backends, since Trove's collector can export to
  several destinations.
- The secretless-loopback behavior is intentional and limited to loopback
  `http://` targets; any internet-facing endpoint still requires a stored
  ingestion key in Sentinel.

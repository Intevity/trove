# Trove sample dashboards

Pre-built dashboards for the three observability backends Trove ships first-class
presets for. Each one renders the same five panels so a user can compare the
shape of their AI-coding telemetry across vendors:

| #   | Panel                         | What it shows                                                                    |
| --- | ----------------------------- | -------------------------------------------------------------------------------- |
| 1   | **Sessions per minute**       | Rate of `*.session.count` increments, broken down by `trove.source`              |
| 2   | **Tool calls per minute**     | Rate of `*.tool.call.count` increments, broken down by `trove.source`            |
| 3   | **Token spend over time**     | Sum of `*.token.usage` (input + output), broken down by `trove.source`           |
| 4   | **Error rate**                | Ratio of `*.error.count` to `*.api_request.count`, broken down by `trove.source` |
| 5   | **Top harnesses by activity** | Table aggregating session count over the time window, grouped by `trove.source`  |

All five panels group on the `trove.source` resource attribute so they work
across every harness Trove supports without per-harness customisation.

## How to import

### Grafana / Grafana Cloud

1. Configure your Prometheus data source so it scrapes the metrics Trove forwards.
2. **Dashboards → New → Import**.
3. Paste the contents of [`grafana-trove.json`](./grafana-trove.json) into the
   "Import via panel JSON" box.
4. Pick your Prometheus data source and click **Import**.

### SigNoz

1. **Dashboards → New dashboard → Import JSON**.
2. Upload [`signoz-trove.json`](./signoz-trove.json).
3. The dashboard binds to the default SigNoz metrics datasource.

### Honeycomb

1. **Boards → New board → Import**.
2. Paste [`honeycomb-trove.json`](./honeycomb-trove.json).
3. Set the dataset to whichever one Trove forwards into (typically `trove` or
   the harness-specific dataset configured in the wizard).

## Notes

- The `trove.source` attribute is emitted directly by Trove's Tier 3 adapters
  (Aider, Cline, Copilot CLI) with the harness id as the value. For Tier 1
  harnesses that emit native OTLP (Claude Code, Gemini CLI, Codex CLI, Qwen
  Code), the value carried on the wire is currently the backend kind — fall
  back to grouping on `service.name` for those and the dashboards still work.
- Metric names follow the OTLP-to-Prometheus name-mangling rules:
  `claude_code.session.count` on the wire becomes `claude_code_session_count`
  in PromQL.
- Tweak the dashboard time range to match your retention; defaults are the
  last 6 hours.

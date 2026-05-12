#!/usr/bin/env node
//
// Trove's Cursor hook — vendored, single-file, stdlib-only.
//
// Cursor invokes this script for each registered hook event. The active
// set today (matching the managed region in cursor_common.rs::build_region):
//
//   - beforeShellExecution / afterShellExecution — terminal commands
//   - beforeSubmitPrompt   / afterAgentResponse  — chat round-trips
//
// **What we emit (Tier A schema — see documentation/architecture.md).**
//
// Every hook invocation produces a *log record* with rich per-event
// detail (event name, conversation/generation ids, command or byte
// counts, model). Logs are unaggregated, high-cardinality is fine.
//
// Only "after" events produce *metric* data points — the matching
// "before" event writes a turn-start marker to /tmp so afterShellExecution
// / afterAgentResponse can compute turn.duration without keeping process
// state (each hook invocation is a fresh node process). Avoiding double
// counts is why before* doesn't emit metrics.
//
// Tier A metrics emitted by this hook:
//
//   - trove.harness.events       Sum (Δ, mono)   event.kind ∈ {chat.turn, shell.exec}
//   - trove.harness.tokens       Sum (Δ, mono)   direction ∈ {input, output}, model
//   - trove.harness.cost.usd     Sum (Δ, mono)   model, cost.method = "estimated"
//   - trove.harness.turn.duration Histogram      event.kind
//   - trove.harness.errors       Sum (Δ, mono)   error.kind ∈ {tool_failure, …}
//
// Cost and tokens are *estimated* — Cursor's hook payload only gives us
// the prompt/response byte length, not the upstream tokenizer's count.
// The `cost.method = estimated` attribute marks every cost point so
// dashboards can filter or weight as desired. Accuracy ~70-90% on
// English/code; see documentation/architecture.md for the tradeoff.
//
// **Metadata only.** This script never emits the textual body of a
// prompt or agent response — only byte counts and opaque ids. Don't
// add body capture here without revisiting the privacy posture
// documented in cursor_common.rs.
//
// To smoke the script outside Cursor, run `cursor-otel-hook.cjs --health`
// — it exits 0 and writes "ok" to stdout. (The wrapper handles `--health`
// itself; this branch exists so the JS impl is independently testable.)

'use strict';

const http = require('node:http');
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const { URL } = require('node:url');

const ENDPOINT = 'http://127.0.0.1:4318';
const TIMEOUT_MS = 1500;

/** Stable harness identifier surfaced on every emission. Cursor IDE and
 *  Cursor CLI share this hook script and a single ~/.cursor/hooks.json
 *  managed region, so the umbrella `cursor` id intentionally collapses
 *  the two — a finer-grained IDE vs. CLI split would need separate
 *  hook commands and breaks the shared-region idempotency invariant
 *  documented in cursor_common.rs. */
const HARNESS_ID = 'cursor';
const HARNESS_NAME = 'Cursor';

/** Per-1k-token USD rates for the cost-estimation path. Keys are
 *  matched against `event.model` by lowercased substring (so
 *  "claude-3-5-sonnet-20241022" and "claude-3.5-sonnet" both map to
 *  the sonnet row). Patterns are checked in order; the first match
 *  wins, so put longer/more-specific names first. When the model is
 *  unknown we skip the cost emission entirely — better to miss a
 *  data point than emit a fabricated number.
 *
 *  Sync the canonical table at documentation/MAPPING_PLAN.md and the
 *  Rust mirror once the mapping-system lands. Prices revisited at each
 *  Trove release. */
const COST_RATES_USD_PER_1K_TOK = [
  // [substring, input_per_1k, output_per_1k]
  ['claude-opus-4', 15.0, 75.0],
  ['claude-sonnet-4', 3.0, 15.0],
  ['claude-haiku-4', 1.0, 5.0],
  ['claude-3-5-sonnet', 3.0, 15.0],
  ['claude-3.5-sonnet', 3.0, 15.0],
  ['claude-3-opus', 15.0, 75.0],
  ['claude-3-haiku', 0.25, 1.25],
  ['gpt-4o-mini', 0.15, 0.6],
  ['gpt-4o', 2.5, 10.0],
  ['gpt-4-turbo', 10.0, 30.0],
  ['o1-mini', 3.0, 12.0],
  ['o1', 15.0, 60.0],
  ['gemini-2.5-pro', 1.25, 10.0],
  ['gemini-2.0-flash', 0.075, 0.3],
  ['gemini-1.5-pro', 1.25, 5.0],
  ['gemini-1.5-flash', 0.075, 0.3],
];

/** Histogram bucket boundaries for trove.harness.turn.duration (seconds).
 *  Covers tab-complete latency through multi-minute agent runs. Match
 *  documentation/architecture.md. */
const TURN_DURATION_BUCKETS_S = [0.5, 1, 2, 5, 10, 20, 30, 60, 120, 300, 600];

if (process.argv.includes('--health')) {
  process.stdout.write('ok\n');
  process.exit(0);
}

let raw = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', (chunk) => {
  raw += chunk;
});
process.stdin.on('end', () => {
  void main(raw).catch(() => {
    // Swallow — the hook must never block or surface errors to Cursor.
    process.exit(0);
  });
});
process.stdin.on('error', () => {
  process.exit(0);
});

/** Cursor hook events Trove cares about, partitioned by response shape.
 *  `gateEvents` expect a `{permission:"allow"}`-style stdout response so
 *  Cursor can decide whether to proceed. `continueEvents` expect a
 *  `{continue:true}` response (different protocol, same intent). Any
 *  other event Cursor invokes the hook for (e.g. afterShellExecution,
 *  afterAgentResponse) ignores stdout — we just record telemetry and
 *  exit. */
const GATE_EVENTS = new Set(['beforeShellExecution']);
const CONTINUE_EVENTS = new Set(['beforeSubmitPrompt']);

/** Map Cursor's raw event names onto the Tier A `event.kind` vocab.
 *  Returns null for events that shouldn't emit a metric (the matching
 *  before* event already wrote a marker; the after* event will emit). */
function tierAEventKind(eventName) {
  switch (eventName) {
    case 'afterAgentResponse':
      return 'chat.turn';
    case 'afterShellExecution':
      return 'shell.exec';
    default:
      return null;
  }
}

async function main(input) {
  let event;
  try {
    event = JSON.parse(input);
  } catch (_err) {
    process.exit(0);
  }
  if (!event || typeof event !== 'object') {
    process.exit(0);
  }

  const eventName = typeof event.hook_event_name === 'string' ? event.hook_event_name : 'unknown';
  const conversationId =
    typeof event.conversation_id === 'string' ? event.conversation_id : 'unknown';
  const generationId = typeof event.generation_id === 'string' ? event.generation_id : 'unknown';
  const cursorVersion = typeof event.cursor_version === 'string' ? event.cursor_version : '';
  const model = typeof event.model === 'string' ? event.model : null;

  const command = typeof event.command === 'string' ? event.command : null;
  const cwd = typeof event.cwd === 'string' ? event.cwd : null;
  const exitCode = typeof event.exit_code === 'number' ? event.exit_code : null;

  // Metadata-only. We read only the LENGTH of textual bodies; bytes go
  // on the wire, the bodies themselves do not.
  const promptBytes =
    typeof event.user_message === 'string' ? byteLength(event.user_message) : null;
  const responseBytes =
    typeof event.agent_message === 'string' ? byteLength(event.agent_message) : null;

  const nowNanos = Date.now() * 1_000_000;
  const nowSeconds = Date.now() / 1000;

  // ---------------- Turn correlation (before* → after* duration) -------
  // Each before* event stamps a marker file keyed by conversation+
  // generation id. The matching after* reads + deletes it to compute
  // the turn duration for the histogram. Best-effort: if the marker
  // is missing (host restart, /tmp cleared) we skip the histogram
  // observation rather than emit a fake number.
  let turnDurationSeconds = null;
  if (eventName === 'beforeShellExecution' || eventName === 'beforeSubmitPrompt') {
    recordTurnStart(conversationId, generationId, nowSeconds);
  } else if (eventName === 'afterShellExecution' || eventName === 'afterAgentResponse') {
    turnDurationSeconds = takeTurnDuration(conversationId, generationId, nowSeconds);
  }

  // ---------------- Log (always emitted, rich detail) ------------------
  const logAttributes = [
    stringAttr('cursor.event', eventName),
    stringAttr('cursor.conversation.id', conversationId),
    stringAttr('cursor.generation.id', generationId),
  ];
  if (cursorVersion !== '') logAttributes.push(stringAttr('cursor.version', cursorVersion));
  if (model !== null) logAttributes.push(stringAttr('cursor.model', model));
  if (command !== null) logAttributes.push(stringAttr('cursor.shell.command', command));
  if (cwd !== null) logAttributes.push(stringAttr('cursor.shell.cwd', cwd));
  if (exitCode !== null) logAttributes.push(intAttr('cursor.shell.exit_code', exitCode));
  if (promptBytes !== null) logAttributes.push(intAttr('cursor.prompt.bytes', promptBytes));
  if (responseBytes !== null) logAttributes.push(intAttr('cursor.response.bytes', responseBytes));
  if (turnDurationSeconds !== null) {
    logAttributes.push(doubleAttr('cursor.turn.duration_seconds', turnDurationSeconds));
  }

  const resourceAttributes = [
    stringAttr('service.name', 'cursor'),
    stringAttr('harness.id', HARNESS_ID),
    stringAttr('harness.name', HARNESS_NAME),
    stringAttr('telemetry.sdk.name', 'trove-cursor-hook'),
    stringAttr('trove.source', 'cursor'),
  ];

  const logsBody = {
    resourceLogs: [
      {
        resource: { attributes: resourceAttributes },
        scopeLogs: [
          {
            scope: { name: 'trove-cursor-hook' },
            logRecords: [
              {
                timeUnixNano: String(nowNanos),
                observedTimeUnixNano: String(nowNanos),
                severityNumber: 9,
                severityText: 'INFO',
                body: { stringValue: `cursor.${eventName}` },
                attributes: logAttributes,
              },
            ],
          },
        ],
      },
    ],
  };

  // ---------------- Tier A metrics (after* events only) -----------------
  const metrics = [];
  const tierAEvent = tierAEventKind(eventName);
  if (tierAEvent !== null) {
    // trove.harness.events — one count per turn.
    metrics.push(
      buildSumMetric(
        'trove.harness.events',
        '1',
        'Count of harness events processed by Trove (one per Cursor turn or shell exec).',
        [
          {
            timeUnixNano: nowNanos,
            attributes: [stringAttr('event.kind', tierAEvent)],
          },
        ],
      ),
    );

    // trove.harness.turn.duration — histogram. Skip if marker was missing.
    if (turnDurationSeconds !== null) {
      metrics.push(
        buildHistogramMetric(
          'trove.harness.turn.duration',
          's',
          'Wall-clock duration of a harness turn (before* → after*).',
          TURN_DURATION_BUCKETS_S,
          [
            {
              timeUnixNano: nowNanos,
              attributes: [stringAttr('event.kind', tierAEvent)],
              value: turnDurationSeconds,
            },
          ],
        ),
      );
    }

    // trove.harness.tokens (estimated from bytes / 4) — chat events only,
    // since shell events have no model billing context.
    if (tierAEvent === 'chat.turn' && model !== null) {
      const points = [];
      if (promptBytes !== null) {
        points.push({
          timeUnixNano: nowNanos,
          value: estimateTokens(promptBytes),
          attributes: [stringAttr('direction', 'input'), stringAttr('model', model)],
        });
      }
      if (responseBytes !== null) {
        points.push({
          timeUnixNano: nowNanos,
          value: estimateTokens(responseBytes),
          attributes: [stringAttr('direction', 'output'), stringAttr('model', model)],
        });
      }
      if (points.length > 0) {
        metrics.push(
          buildSumMetric(
            'trove.harness.tokens',
            '{token}',
            'Estimated token volume per harness turn (bytes/4 heuristic for hook-only harnesses).',
            points,
          ),
        );
      }
    }

    // trove.harness.cost.usd — chat events only, requires known model.
    if (tierAEvent === 'chat.turn' && model !== null) {
      const rate = lookupRate(model);
      if (rate !== null && (promptBytes !== null || responseBytes !== null)) {
        const inputTok = promptBytes !== null ? estimateTokens(promptBytes) : 0;
        const outputTok = responseBytes !== null ? estimateTokens(responseBytes) : 0;
        const costUsd = (inputTok / 1000) * rate[0] + (outputTok / 1000) * rate[1];
        metrics.push(
          buildSumDoubleMetric(
            'trove.harness.cost.usd',
            'USD',
            'Estimated dollar cost per harness turn (rate table × estimated tokens).',
            [
              {
                timeUnixNano: nowNanos,
                value: costUsd,
                attributes: [stringAttr('model', model), stringAttr('cost.method', 'estimated')],
              },
            ],
          ),
        );
      }
    }

    // trove.harness.errors — shell exit_code != 0 → tool_failure.
    if (tierAEvent === 'shell.exec' && exitCode !== null && exitCode !== 0) {
      metrics.push(
        buildSumMetric(
          'trove.harness.errors',
          '1',
          'Count of error-class events observed in a harness.',
          [
            {
              timeUnixNano: nowNanos,
              attributes: [stringAttr('error.kind', 'tool_failure')],
            },
          ],
        ),
      );
    }
  }

  const posts = [postJson(`${ENDPOINT}/v1/logs`, logsBody)];
  if (metrics.length > 0) {
    const metricsBody = {
      resourceMetrics: [
        {
          resource: { attributes: resourceAttributes },
          scopeMetrics: [{ scope: { name: 'trove-cursor-hook' }, metrics }],
        },
      ],
    };
    posts.push(postJson(`${ENDPOINT}/v1/metrics`, metricsBody));
  }
  await Promise.all(posts);

  // Cursor's gate-style protocol: emit the permissive response shape
  // that matches the event's expected reply. Pulled from Cursor's own
  // bundled `W2t`/`QHy` helpers (workbench.desktop.main.js): shell-exec
  // gates take `{permission:"allow"}`; the prompt-submit gate takes
  // `{continue:true}`. Post-execution events ignore stdout entirely.
  if (GATE_EVENTS.has(eventName)) {
    process.stdout.write(JSON.stringify({ permission: 'allow' }) + '\n');
  } else if (CONTINUE_EVENTS.has(eventName)) {
    process.stdout.write(JSON.stringify({ continue: true }) + '\n');
  }
  process.exit(0);
}

// ---------------- OTLP helpers ----------------------------------------

function stringAttr(key, value) {
  return { key, value: { stringValue: String(value) } };
}

function intAttr(key, value) {
  return { key, value: { intValue: String(value) } };
}

function doubleAttr(key, value) {
  return { key, value: { doubleValue: value } };
}

/** Build a Sum (Δ, monotonic, intValue) metric from N data points. */
function buildSumMetric(name, unit, description, points) {
  return {
    name,
    unit,
    description,
    sum: {
      aggregationTemporality: 1,
      isMonotonic: true,
      dataPoints: points.map((p) => ({
        startTimeUnixNano: String(p.timeUnixNano),
        timeUnixNano: String(p.timeUnixNano),
        asInt: String(p.value !== undefined ? p.value : 1),
        attributes: p.attributes,
      })),
    },
  };
}

/** Build a Sum (Δ, monotonic, doubleValue) metric — for cost/USD. */
function buildSumDoubleMetric(name, unit, description, points) {
  return {
    name,
    unit,
    description,
    sum: {
      aggregationTemporality: 1,
      isMonotonic: true,
      dataPoints: points.map((p) => ({
        startTimeUnixNano: String(p.timeUnixNano),
        timeUnixNano: String(p.timeUnixNano),
        asDouble: p.value,
        attributes: p.attributes,
      })),
    },
  };
}

/** Build a Histogram metric. Each data point is a single observation
 *  placed in the matching bucket; OTLP records the explicit bounds
 *  alongside per-bucket counts (length = bounds.length + 1). */
function buildHistogramMetric(name, unit, description, bounds, points) {
  return {
    name,
    unit,
    description,
    histogram: {
      aggregationTemporality: 1,
      dataPoints: points.map((p) => {
        const bucketCounts = new Array(bounds.length + 1).fill('0');
        let placed = false;
        for (let i = 0; i < bounds.length; i++) {
          if (p.value <= bounds[i]) {
            bucketCounts[i] = '1';
            placed = true;
            break;
          }
        }
        if (!placed) bucketCounts[bounds.length] = '1';
        return {
          startTimeUnixNano: String(p.timeUnixNano),
          timeUnixNano: String(p.timeUnixNano),
          count: '1',
          sum: p.value,
          bucketCounts,
          explicitBounds: bounds,
          attributes: p.attributes,
        };
      }),
    },
  };
}

// ---------------- Estimation + rate lookup ----------------------------

/** Token estimate from UTF-8 byte length. ~4 bytes/token is the
 *  industry-standard rough heuristic for English/code mixed text.
 *  Returns a non-negative integer. */
function estimateTokens(bytes) {
  return Math.max(0, Math.ceil(bytes / 4));
}

/** Look up [input_per_1k_usd, output_per_1k_usd] for a model name by
 *  lowercased substring match against the rate table. Returns null if
 *  no entry matches — cost emission is skipped in that case. */
function lookupRate(model) {
  const lower = model.toLowerCase();
  for (const row of COST_RATES_USD_PER_1K_TOK) {
    if (lower.includes(row[0])) return [row[1], row[2]];
  }
  return null;
}

// ---------------- Turn correlation (before* → after*) -----------------

/** Per-host marker directory. We use a single directory under tmpdir
 *  so each (conversation, generation) pair gets one tiny file. The
 *  directory is created lazily and never cleaned up wholesale —
 *  individual markers are deleted when the matching after* fires.
 *  Orphaned markers (host restart, /tmp cleared) eventually time out
 *  via the staleness check in takeTurnDuration. */
const TURN_DIR = path.join(os.tmpdir(), 'trove-cursor-turns');

/** Sanitize an id into a filesystem-safe basename. Cursor's ids are
 *  typically UUIDs but we defensively strip anything not in [A-Za-z0-9-]
 *  before joining the path so a hostile id can't path-traverse. */
function safeId(id) {
  return String(id)
    .replace(/[^A-Za-z0-9_.-]/g, '_')
    .slice(0, 128);
}

function markerPath(conversationId, generationId) {
  return path.join(TURN_DIR, `${safeId(conversationId)}__${safeId(generationId)}.t`);
}

function recordTurnStart(conversationId, generationId, nowSeconds) {
  try {
    fs.mkdirSync(TURN_DIR, { recursive: true });
    fs.writeFileSync(markerPath(conversationId, generationId), String(nowSeconds), 'utf8');
  } catch (_err) {
    // Marker write failed — turn.duration will be skipped for this
    // pair. Logs+counter still emit; the histogram is best-effort.
  }
}

/** Read+delete the marker for a (conversation, generation) pair and
 *  return the elapsed wall-clock seconds. Returns null if the marker
 *  is missing or older than 1 hour (probably an orphan from before a
 *  /tmp wipe or a crashed turn). */
function takeTurnDuration(conversationId, generationId, nowSeconds) {
  const p = markerPath(conversationId, generationId);
  let startSeconds = null;
  try {
    const raw = fs.readFileSync(p, 'utf8');
    const parsed = Number(raw);
    if (Number.isFinite(parsed)) startSeconds = parsed;
  } catch (_err) {
    return null;
  }
  try {
    fs.unlinkSync(p);
  } catch (_err) {
    // Best-effort cleanup. If unlink fails we'll just re-overwrite
    // the marker on the next before* with the same ids.
  }
  if (startSeconds === null) return null;
  const elapsed = nowSeconds - startSeconds;
  if (!Number.isFinite(elapsed) || elapsed < 0 || elapsed > 3600) {
    // Negative clock skew or > 1h stale → treat as missing.
    return null;
  }
  return elapsed;
}

// ---------------- HTTP -------------------------------------------------

/** UTF-8 byte length of a JS string. Buffer.byteLength is exact and
 *  cheap; matches what `Content-Length` would report if the body were
 *  serialised. We use bytes (not codepoints) so a multilingual prompt
 *  isn't undercounted on dashboards. */
function byteLength(str) {
  return Buffer.byteLength(str, 'utf8');
}

function postJson(urlString, body) {
  return new Promise((resolve) => {
    let url;
    try {
      url = new URL(urlString);
    } catch (_err) {
      resolve();
      return;
    }
    const payload = Buffer.from(JSON.stringify(body), 'utf8');
    const options = {
      method: 'POST',
      hostname: url.hostname,
      port: url.port || (url.protocol === 'https:' ? 443 : 80),
      path: url.pathname + url.search,
      headers: {
        'content-type': 'application/json',
        'content-length': String(payload.length),
      },
      timeout: TIMEOUT_MS,
    };

    const req = http.request(options, (res) => {
      res.on('data', () => {});
      res.on('end', () => resolve());
      res.on('error', () => resolve());
    });
    req.on('error', () => resolve());
    req.on('timeout', () => {
      req.destroy();
      resolve();
    });
    req.write(payload);
    req.end();
  });
}

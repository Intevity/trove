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
// **Rules-driven emission.** The script reads the user's current Trove
// mapping state from `~/.cursor/trove-hook-rules.json` (written by the
// Tauri app's `apply_mappings` IPC). For each Cursor hook event the
// script classifies the observation into one or more `when` keys (one
// per facet: events, tokens.input, tokens.output, cost, error) and looks
// up every rule whose `when` matches. The rule's target metric id is
// resolved against the sidecar's catalog to produce the wire name and
// OTLP shape. This is the same rules-driven path the in-process Rust
// watchers (Cline, Gemini, Claude Desktop) use — the JS port keeps
// Cursor on the same model.
//
// **Fallback.** When the sidecar is missing (host upgraded the bundled
// cjs before the app's first apply_mappings) we drive the same default
// rules in-memory (see `FALLBACK_SIDECAR`). The defaults mirror the
// Rust `cursor_ide_defaults()` row-by-row so the emit shape is identical
// either way.
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
 *  data point than emit a fabricated number. */
const COST_RATES_USD_PER_1K_TOK = [
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
 *  Covers tab-complete latency through multi-minute agent runs. Must
 *  match `DEFAULT_HISTOGRAM_BUCKET_BOUNDS_SECONDS` in the Rust runtime
 *  so cross-harness duration panels render against the same buckets. */
const TURN_DURATION_BUCKETS_S = [0.5, 1, 2, 5, 10, 20, 30, 60, 120, 300, 600];

/** Sidecar file the Tauri app writes whenever the user changes mappings.
 *  The script reads this every invocation; on miss we use the embedded
 *  fallback (which is byte-for-byte equivalent to a fresh install). */
const SIDECAR_PATH = path.join(os.homedir(), '.cursor', 'trove-hook-rules.json');

/** Builtin catalog entries + cursor rules embedded inline. This mirrors
 *  the Rust defaults exactly so a host that hasn't yet seen an
 *  `apply_mappings` call still emits the canonical Tier A set. Schema
 *  version is bumped in lockstep with `SIDECAR_SCHEMA_VERSION` in
 *  `cursor_hook_codegen.rs`. */
const FALLBACK_SIDECAR = {
  schemaVersion: 1,
  metrics: [
    {
      id: 'events',
      name: 'trove.harness.events',
      kind: 'counter',
      requiredAttributes: ['event.kind'],
    },
    {
      id: 'tokens',
      name: 'trove.harness.tokens',
      kind: 'counter',
      requiredAttributes: ['direction'],
    },
    {
      id: 'cost.usd',
      name: 'trove.harness.cost.usd',
      kind: 'counter',
      requiredAttributes: ['cost.method'],
    },
    {
      id: 'turn.duration',
      name: 'trove.harness.turn.duration',
      kind: 'histogram',
      requiredAttributes: [],
    },
    {
      id: 'errors',
      name: 'trove.harness.errors',
      kind: 'counter',
      requiredAttributes: ['error.kind'],
    },
  ],
  rules: [
    { when: 'beforeSubmitPrompt', emit: null },
    { when: 'beforeShellExecution', emit: null },
    {
      when: 'afterAgentResponse',
      emit: { metric: 'events', attributes: { 'event.kind': 'chat.turn' } },
    },
    {
      when: 'afterAgentResponse',
      emit: { metric: 'turn.duration', attributes: { 'event.kind': 'chat.turn' } },
    },
    {
      when: 'afterAgentResponse.tokens.input',
      emit: { metric: 'tokens', attributes: { direction: 'input' } },
    },
    {
      when: 'afterAgentResponse.tokens.output',
      emit: { metric: 'tokens', attributes: { direction: 'output' } },
    },
    {
      when: 'afterAgentResponse.cost',
      emit: { metric: 'cost.usd', attributes: { 'cost.method': 'estimated' } },
    },
    {
      when: 'afterShellExecution',
      emit: { metric: 'events', attributes: { 'event.kind': 'shell.exec' } },
    },
    {
      when: 'afterShellExecution',
      emit: { metric: 'turn.duration', attributes: { 'event.kind': 'shell.exec' } },
    },
    {
      when: 'afterShellExecution.error',
      emit: { metric: 'errors', attributes: { 'error.kind': 'tool_failure' } },
    },
  ],
};

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

/** Load the sidecar; fall back to the embedded defaults on any error.
 *  We don't validate aggressively here — the Rust side has already
 *  validated and serialized the catalog. We only check that the file
 *  parses as JSON, has a recognizable schemaVersion, and has the two
 *  fields we read. */
function loadSidecar() {
  try {
    const text = fs.readFileSync(SIDECAR_PATH, 'utf8');
    const parsed = JSON.parse(text);
    if (
      parsed &&
      typeof parsed === 'object' &&
      typeof parsed.schemaVersion === 'number' &&
      parsed.schemaVersion <= 1 &&
      Array.isArray(parsed.metrics) &&
      Array.isArray(parsed.rules)
    ) {
      return parsed;
    }
  } catch (_err) {
    // sidecar missing or malformed → fallback
  }
  return FALLBACK_SIDECAR;
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

  // ---------------- Tier A metrics — rules-driven ----------------------
  const sidecar = loadSidecar();
  const catalog = new Map(sidecar.metrics.map((m) => [m.id, m]));
  const rules = sidecar.rules;
  const acc = makeAccumulator(catalog, rules);

  if (eventName === 'afterAgentResponse') {
    observeCount(acc, 'afterAgentResponse', 1, {});
    if (turnDurationSeconds !== null) {
      observeHistogram(acc, 'afterAgentResponse', turnDurationSeconds, {});
    }
    if (model !== null) {
      if (promptBytes !== null) {
        observeCount(acc, 'afterAgentResponse.tokens.input', estimateTokens(promptBytes), {
          model,
        });
      }
      if (responseBytes !== null) {
        observeCount(acc, 'afterAgentResponse.tokens.output', estimateTokens(responseBytes), {
          model,
        });
      }
      const rate = lookupRate(model);
      if (rate !== null && (promptBytes !== null || responseBytes !== null)) {
        const inputTok = promptBytes !== null ? estimateTokens(promptBytes) : 0;
        const outputTok = responseBytes !== null ? estimateTokens(responseBytes) : 0;
        const costUsd = (inputTok / 1000) * rate[0] + (outputTok / 1000) * rate[1];
        observeDoubleSum(acc, 'afterAgentResponse.cost', costUsd, { model });
      }
    }
  } else if (eventName === 'afterShellExecution') {
    observeCount(acc, 'afterShellExecution', 1, {});
    if (turnDurationSeconds !== null) {
      observeHistogram(acc, 'afterShellExecution', turnDurationSeconds, {});
    }
    if (exitCode !== null && exitCode !== 0) {
      observeCount(acc, 'afterShellExecution.error', 1, {});
    }
  }

  const metrics = buildMetricsFromAccumulator(acc, nowNanos);

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
  // that matches the event's expected reply.
  if (GATE_EVENTS.has(eventName)) {
    process.stdout.write(JSON.stringify({ permission: 'allow' }) + '\n');
  } else if (CONTINUE_EVENTS.has(eventName)) {
    process.stdout.write(JSON.stringify({ continue: true }) + '\n');
  }
  process.exit(0);
}

// ---------------- Rules-driven accumulator ----------------------------
//
// Mirrors the structure of `mappings::runtime::MetricsAccumulator` in
// Rust. Per-observation extras merge over the rule's static attributes
// (observation-wins). Each (metricId, signature) tuple buckets one data
// point. At build time, the bucket shape (Sum/Histogram/etc.) is decided
// by the observed values, not just the catalog kind — that's how cost
// counters and int counters can share a "counter" kind in the catalog.

function makeAccumulator(catalog, rules) {
  return {
    catalog,
    rules,
    buckets: new Map(),
    order: [],
  };
}

function matchingRules(acc, when, kindFilter) {
  const out = [];
  for (const r of acc.rules) {
    if (r.when !== when) continue;
    if (!r.emit) continue;
    const def = acc.catalog.get(r.emit.metric);
    if (!def) continue;
    if (!kindFilter(def.kind)) continue;
    out.push({ metricId: r.emit.metric, ruleAttrs: r.emit.attributes || {}, kind: def.kind });
  }
  return out;
}

function observeCount(acc, when, count, extras) {
  if (count === 0) return;
  const matches = matchingRules(acc, when, (k) => k === 'counter' || k === 'gauge');
  for (const m of matches) {
    const sig = mergeAttrs(m.ruleAttrs, extras);
    const key = bucketKey(m.metricId, sig);
    let b = acc.buckets.get(key);
    if (!b) {
      b = {
        metricId: m.metricId,
        sig,
        type: 'counter_int',
        intValue: 0,
        doubleValue: 0,
        samples: [],
      };
      acc.buckets.set(key, b);
      acc.order.push(key);
    }
    if (b.type === 'gauge') {
      b.doubleValue = count;
    } else {
      b.intValue += count;
    }
  }
}

function observeHistogram(acc, when, seconds, extras) {
  if (!Number.isFinite(seconds) || seconds < 0) return;
  const matches = matchingRules(acc, when, (k) => k === 'histogram');
  for (const m of matches) {
    const sig = mergeAttrs(m.ruleAttrs, extras);
    const key = bucketKey(m.metricId, sig);
    let b = acc.buckets.get(key);
    if (!b) {
      b = {
        metricId: m.metricId,
        sig,
        type: 'histogram',
        intValue: 0,
        doubleValue: 0,
        samples: [],
      };
      acc.buckets.set(key, b);
      acc.order.push(key);
    }
    b.samples.push(seconds);
  }
}

function observeDoubleSum(acc, when, value, extras) {
  if (!Number.isFinite(value)) return;
  const matches = matchingRules(acc, when, (k) => k === 'counter' || k === 'gauge');
  for (const m of matches) {
    const sig = mergeAttrs(m.ruleAttrs, extras);
    const key = bucketKey(m.metricId, sig);
    let b = acc.buckets.get(key);
    if (!b) {
      b = {
        metricId: m.metricId,
        sig,
        type: 'counter_double',
        intValue: 0,
        doubleValue: 0,
        samples: [],
      };
      acc.buckets.set(key, b);
      acc.order.push(key);
    }
    if (b.type === 'counter_int') {
      // Promote to double if any observation was double.
      b.type = 'counter_double';
      b.doubleValue = b.intValue + value;
      b.intValue = 0;
    } else {
      b.doubleValue += value;
    }
  }
}

function buildMetricsFromAccumulator(acc, nowNanos) {
  // Group buckets by metric id (preserving first-insertion order).
  const grouped = new Map();
  for (const key of acc.order) {
    const b = acc.buckets.get(key);
    if (!b) continue;
    if (!grouped.has(b.metricId)) grouped.set(b.metricId, []);
    grouped.get(b.metricId).push(b);
  }

  const out = [];
  for (const [metricId, buckets] of grouped) {
    const def = acc.catalog.get(metricId);
    if (!def) continue;

    const anyHist = buckets.some((b) => b.type === 'histogram');
    const anyDouble = buckets.some((b) => b.type === 'counter_double');

    if (anyHist) {
      out.push(buildHistogramFromBuckets(def, buckets, nowNanos));
    } else if (anyDouble) {
      out.push(buildSumFromBuckets(def, buckets, nowNanos, /*asDouble=*/ true));
    } else {
      out.push(buildSumFromBuckets(def, buckets, nowNanos, /*asDouble=*/ false));
    }
  }
  return out;
}

function buildSumFromBuckets(def, buckets, nowNanos, asDouble) {
  const dataPoints = buckets.map((b) => {
    const point = {
      startTimeUnixNano: String(nowNanos),
      timeUnixNano: String(nowNanos),
      attributes: sigToAttrs(b.sig),
    };
    if (asDouble) {
      point.asDouble = b.type === 'counter_double' ? b.doubleValue : b.intValue;
    } else {
      point.asInt = String(b.intValue);
    }
    return point;
  });
  return {
    name: def.name,
    unit: metricUnit(def.id),
    description: metricDescription(def),
    sum: {
      aggregationTemporality: 1,
      isMonotonic: true,
      dataPoints,
    },
  };
}

function buildHistogramFromBuckets(def, buckets, nowNanos) {
  const bounds = TURN_DURATION_BUCKETS_S;
  const dataPoints = buckets.map((b) => {
    const samples = b.samples || [];
    const count = samples.length;
    const sum = samples.reduce((a, s) => a + s, 0);
    const bucketCounts = new Array(bounds.length + 1).fill(0);
    for (const s of samples) {
      let placed = false;
      for (let i = 0; i < bounds.length; i++) {
        if (s <= bounds[i]) {
          bucketCounts[i] += 1;
          placed = true;
          break;
        }
      }
      if (!placed) bucketCounts[bounds.length] += 1;
    }
    return {
      startTimeUnixNano: String(nowNanos),
      timeUnixNano: String(nowNanos),
      count: String(count),
      sum,
      bucketCounts: bucketCounts.map(String),
      explicitBounds: bounds,
      attributes: sigToAttrs(b.sig),
    };
  });
  return {
    name: def.name,
    unit: 's',
    description: metricDescription(def),
    histogram: {
      aggregationTemporality: 1,
      dataPoints,
    },
  };
}

function metricUnit(id) {
  if (id === 'cost.usd') return 'USD';
  if (id === 'turn.duration') return 's';
  return '1';
}

function metricDescription(def) {
  switch (def.id) {
    case 'events':
      return 'Count of harness events processed by Trove.';
    case 'tokens':
      return 'Token usage by direction.';
    case 'cost.usd':
      return 'Estimated USD cost.';
    case 'turn.duration':
      return 'Per-turn duration in seconds.';
    case 'errors':
      return 'Count of harness errors observed by Trove.';
    default:
      return `Custom metric ${def.name}.`;
  }
}

function mergeAttrs(ruleAttrs, extras) {
  // Observation-wins: extras override the rule's static inject for
  // duplicate keys.
  const merged = {};
  for (const k of Object.keys(ruleAttrs || {})) merged[k] = ruleAttrs[k];
  for (const k of Object.keys(extras || {})) merged[k] = extras[k];
  return merged;
}

function bucketKey(metricId, sig) {
  const keys = Object.keys(sig).sort();
  let s = metricId;
  for (const k of keys) s += `\x00${k}\x01${sig[k]}`;
  return s;
}

function sigToAttrs(sig) {
  return Object.keys(sig)
    .sort()
    .map((k) => stringAttr(k, sig[k]));
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
 *  so each (conversation, generation) pair gets one tiny file. */
const TURN_DIR = path.join(os.tmpdir(), 'trove-cursor-turns');

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
    // Best-effort cleanup.
  }
  if (startSeconds === null) return null;
  const elapsed = nowSeconds - startSeconds;
  if (!Number.isFinite(elapsed) || elapsed < 0 || elapsed > 3600) {
    return null;
  }
  return elapsed;
}

// ---------------- HTTP -------------------------------------------------

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

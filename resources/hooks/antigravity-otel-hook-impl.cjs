#!/usr/bin/env node
//
// Trove's Antigravity CLI (`agy`) hook — vendored, single-file, stdlib-only.
//
// Antigravity invokes this script for each registered hook event. The
// active set today (matching the managed region in
// antigravity_cli.rs::build_region):
//
//   - UserPromptSubmit / Stop                      — chat round-trips
//   - BeforeShellExecution / AfterShellExecution   — terminal commands
//   - ErrorOccurred                                — agent-level errors
//
// Antigravity dropped the native OTLP exporter Gemini CLI had, but
// inherited Gemini CLI's Claude-Code-style **Hooks** mechanism. This
// script is the bridge: it turns each agent event into Trove's Tier A
// OTLP metrics, posted directly to the local collector — exactly the
// model the Cursor hook uses (Cursor also lacks native OTLP).
//
// **Rules-driven emission.** The script reads the user's current Trove
// mapping state from `~/.gemini/antigravity-cli/trove-hook-rules.json`
// (written by the Tauri app's `apply_mappings` IPC). For each agy hook
// event the script classifies the observation into one or more `when`
// keys (one per facet: events, tokens.input, tokens.output, cost, error)
// and looks up every rule whose `when` matches. The rule's target metric
// id is resolved against the sidecar's catalog to produce the wire name
// and OTLP shape.
//
// **Fallback.** When the sidecar is missing (host upgraded the bundled
// cjs before the app's first apply_mappings) we drive the same default
// rules in-memory (see `FALLBACK_SIDECAR`). The defaults mirror the
// Rust `antigravity_cli_defaults()` row-by-row so the emit shape is
// identical either way.
//
// **Field mapping is defensive.** agy's exact event payload (precise
// field names, whether Stop carries token usage / response text) can
// only be confirmed against a live, logged-in interactive session, so
// the extractors below probe several candidate field names and degrade
// gracefully: `events` + `turn.duration` always emit; tokens/cost emit
// only when the corresponding byte/usage data is present. Input-token
// estimation reads the prompt size stashed at UserPromptSubmit.
//
// **Metadata only.** This script never emits the textual body of a
// prompt or agent response — only byte counts and opaque ids. Don't
// add body capture here without revisiting the privacy posture
// documented in antigravity_cli.rs.
//
// To smoke the script run `antigravity-otel-hook.cjs --health` — it exits
// 0 and writes "ok" to stdout. (The wrapper handles `--health` itself;
// this branch exists so the JS impl is independently testable.)

'use strict';

const http = require('node:http');
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const { URL } = require('node:url');

const ENDPOINT = 'http://127.0.0.1:4318';
const TIMEOUT_MS = 1500;

/** Stable harness identifier surfaced on every emission. Set inline in
 *  the OTLP resource attributes so the collector needs no harness-tag /
 *  tierA / diag overlay for this harness (it self-tags, exactly like the
 *  Cursor hook — see `native_service_name_candidates` returning `&[]`). */
const HARNESS_ID = 'antigravity-cli';
const HARNESS_NAME = 'Antigravity CLI';

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
  // Gemini 3 family — the models Antigravity CLI runs by default. Longer
  // / more-specific names first so "gemini-3-pro-preview" matches the pro
  // row before any shorter substring. Rates are public list prices per
  // 1k tokens; refresh when Google revises the pricing page.
  ['gemini-3-pro', 2.0, 12.0],
  ['gemini-3-flash', 0.1, 0.4],
  ['gemini-3', 2.0, 12.0],
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
const SIDECAR_PATH = path.join(os.homedir(), '.gemini', 'antigravity-cli', 'trove-hook-rules.json');

/** Builtin catalog entries + antigravity rules embedded inline. This
 *  mirrors the Rust `antigravity_cli_defaults()` exactly so a host that
 *  hasn't yet seen an `apply_mappings` call still emits the canonical
 *  Tier A set. Schema version is bumped in lockstep with
 *  `SIDECAR_SCHEMA_VERSION` in `cursor_hook_codegen.rs`. */
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
    // Turn-start markers — suppressed so they don't double-count. The
    // hook stashes start time + prompt size keyed by session so the
    // matching Stop / AfterShellExecution can compute duration + tokens.
    { when: 'UserPromptSubmit', emit: null },
    { when: 'BeforeShellExecution', emit: null },
    {
      when: 'Stop',
      emit: { metric: 'events', attributes: { 'event.kind': 'chat.turn' } },
    },
    {
      when: 'Stop',
      emit: { metric: 'turn.duration', attributes: { 'event.kind': 'chat.turn' } },
    },
    {
      when: 'Stop.tokens.input',
      emit: { metric: 'tokens', attributes: { direction: 'input' } },
    },
    {
      when: 'Stop.tokens.output',
      emit: { metric: 'tokens', attributes: { direction: 'output' } },
    },
    {
      when: 'Stop.cost',
      emit: { metric: 'cost.usd', attributes: { 'cost.method': 'estimated' } },
    },
    {
      when: 'AfterShellExecution',
      emit: { metric: 'events', attributes: { 'event.kind': 'shell.exec' } },
    },
    {
      when: 'AfterShellExecution',
      emit: { metric: 'turn.duration', attributes: { 'event.kind': 'shell.exec' } },
    },
    {
      when: 'AfterShellExecution.error',
      emit: { metric: 'errors', attributes: { 'error.kind': 'tool_failure' } },
    },
    {
      when: 'ErrorOccurred',
      emit: { metric: 'errors', attributes: { 'error.kind': 'unknown' } },
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
    // Swallow — the hook must never block or surface errors to agy.
    process.exit(0);
  });
});
process.stdin.on('error', () => {
  process.exit(0);
});

/** agy's gate-response protocol for before-events (BeforeShellExecution)
 *  isn't confirmed against a live session yet, and emitting a wrongly
 *  shaped verdict risks blocking the user's shell. A `command` hook that
 *  exits 0 with empty stdout is the protocol-agnostic "proceed" signal,
 *  so Trove stays silent on every event and only records telemetry. If a
 *  future rev needs to gate, populate this set and emit the verdict shape
 *  agy expects. */
const GATE_EVENTS = new Set();

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

  // agy's payload uses Claude-Code-style snake_case names; probe a few
  // candidates per field so we tolerate minor schema drift across `agy`
  // revs without an interactive re-confirmation.
  const eventName = pickString(event, ['hook_event_name', 'eventName', 'event']) || 'unknown';
  const sessionId = pickString(event, ['session_id', 'sessionId', 'conversation_id']) || 'unknown';
  const model = pickString(event, ['model', 'model_name', 'modelId']);

  // Shell-exec fields (Before/AfterShellExecution). The command may be
  // flat (`command`) or nested under a Claude-Code-style `tool_input`.
  const command =
    pickString(event, ['command']) ||
    (event.tool_input ? pickString(event.tool_input, ['command']) : null);
  const cwd = pickString(event, ['cwd', 'workspace', 'workspace_path']);
  const exitCode =
    pickNumber(event, ['exit_code', 'exitCode']) ??
    (event.tool_response ? pickNumber(event.tool_response, ['exit_code', 'exitCode']) : null);

  // Metadata-only. We read only the LENGTH of any textual body present;
  // the bytes go on the wire, the bodies themselves never do.
  const promptText = pickString(event, ['prompt', 'user_message', 'message', 'user_prompt']);
  const promptBytesNow = promptText !== null ? byteLength(promptText) : null;
  const responseText = pickString(event, [
    'response',
    'agent_message',
    'last_message',
    'assistant_message',
  ]);
  const responseBytes = responseText !== null ? byteLength(responseText) : null;

  // Exact token usage, if agy surfaces it (preferred over estimation).
  const usage = event.usage && typeof event.usage === 'object' ? event.usage : event;
  const exactInputTokens = pickNumber(usage, ['input_tokens', 'prompt_tokens', 'inputTokens']);
  const exactOutputTokens = pickNumber(usage, [
    'output_tokens',
    'completion_tokens',
    'outputTokens',
  ]);

  const nowNanos = Date.now() * 1_000_000;
  const nowSeconds = Date.now() / 1000;

  // ---------------- Turn correlation (start marker → terminal event) ----
  // UserPromptSubmit/BeforeShellExecution fire in a separate process from
  // their matching Stop/AfterShellExecution, so we stash the start time
  // (and, for chat turns, the prompt size for input-token estimation) in
  // a per-session marker file. `chat` and `shell` use distinct slots so
  // an in-flight shell command can't clobber the chat-turn marker.
  let turnDurationSeconds = null;
  let stashedPromptBytes = null;
  if (eventName === 'UserPromptSubmit') {
    recordTurnStart(sessionId, 'chat', nowSeconds, promptBytesNow);
  } else if (eventName === 'BeforeShellExecution') {
    recordTurnStart(sessionId, 'shell', nowSeconds, null);
  } else if (eventName === 'Stop') {
    const t = takeTurn(sessionId, 'chat', nowSeconds);
    turnDurationSeconds = t.durationSeconds;
    stashedPromptBytes = t.promptBytes;
  } else if (eventName === 'AfterShellExecution') {
    const t = takeTurn(sessionId, 'shell', nowSeconds);
    turnDurationSeconds = t.durationSeconds;
  }

  // Effective input size for the turn: a prompt body on the Stop event
  // (rare) wins, else the size we stashed at UserPromptSubmit.
  const inputBytes = promptBytesNow !== null ? promptBytesNow : stashedPromptBytes;

  // ---------------- Log (always emitted, rich detail) ------------------
  const logAttributes = [
    stringAttr('antigravity.event', eventName),
    stringAttr('antigravity.session.id', sessionId),
  ];
  if (model !== null) logAttributes.push(stringAttr('antigravity.model', model));
  if (command !== null) logAttributes.push(stringAttr('antigravity.shell.command', command));
  if (cwd !== null) logAttributes.push(stringAttr('antigravity.shell.cwd', cwd));
  if (exitCode !== null) logAttributes.push(intAttr('antigravity.shell.exit_code', exitCode));
  if (inputBytes !== null) logAttributes.push(intAttr('antigravity.prompt.bytes', inputBytes));
  if (responseBytes !== null) {
    logAttributes.push(intAttr('antigravity.response.bytes', responseBytes));
  }
  if (turnDurationSeconds !== null) {
    logAttributes.push(doubleAttr('antigravity.turn.duration_seconds', turnDurationSeconds));
  }

  const resourceAttributes = [
    stringAttr('service.name', 'antigravity-cli'),
    stringAttr('harness.id', HARNESS_ID),
    stringAttr('harness.name', HARNESS_NAME),
    stringAttr('telemetry.sdk.name', 'trove-antigravity-hook'),
    stringAttr('trove.source', 'antigravity-cli'),
  ];

  const logsBody = {
    resourceLogs: [
      {
        resource: { attributes: resourceAttributes },
        scopeLogs: [
          {
            scope: { name: 'trove-antigravity-hook' },
            logRecords: [
              {
                timeUnixNano: String(nowNanos),
                observedTimeUnixNano: String(nowNanos),
                severityNumber: 9,
                severityText: 'INFO',
                body: { stringValue: `antigravity.${eventName}` },
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

  if (eventName === 'Stop') {
    // One chat turn completed.
    observeCount(acc, 'Stop', 1, {});
    if (turnDurationSeconds !== null) {
      observeHistogram(acc, 'Stop', turnDurationSeconds, {});
    }
    const modelExtra = model !== null ? { model } : {};
    // Prefer exact usage counts when agy supplies them; otherwise fall
    // back to a ~4 bytes/token estimate from the prompt/response sizes.
    const inputTok =
      exactInputTokens !== null
        ? exactInputTokens
        : inputBytes !== null
          ? estimateTokens(inputBytes)
          : null;
    const outputTok =
      exactOutputTokens !== null
        ? exactOutputTokens
        : responseBytes !== null
          ? estimateTokens(responseBytes)
          : null;
    if (inputTok !== null) {
      observeCount(acc, 'Stop.tokens.input', inputTok, modelExtra);
    }
    if (outputTok !== null) {
      observeCount(acc, 'Stop.tokens.output', outputTok, modelExtra);
    }
    if (model !== null) {
      const rate = lookupRate(model);
      if (rate !== null && (inputTok !== null || outputTok !== null)) {
        const costUsd = ((inputTok || 0) / 1000) * rate[0] + ((outputTok || 0) / 1000) * rate[1];
        observeDoubleSum(acc, 'Stop.cost', costUsd, { model });
      }
    }
  } else if (eventName === 'AfterShellExecution') {
    observeCount(acc, 'AfterShellExecution', 1, {});
    if (turnDurationSeconds !== null) {
      observeHistogram(acc, 'AfterShellExecution', turnDurationSeconds, {});
    }
    if (exitCode !== null && exitCode !== 0) {
      observeCount(acc, 'AfterShellExecution.error', 1, {});
    }
  } else if (eventName === 'ErrorOccurred') {
    observeCount(acc, 'ErrorOccurred', 1, {});
  }

  const metrics = buildMetricsFromAccumulator(acc, nowNanos);

  const posts = [postJson(`${ENDPOINT}/v1/logs`, logsBody)];
  if (metrics.length > 0) {
    const metricsBody = {
      resourceMetrics: [
        {
          resource: { attributes: resourceAttributes },
          scopeMetrics: [{ scope: { name: 'trove-antigravity-hook' }, metrics }],
        },
      ],
    };
    posts.push(postJson(`${ENDPOINT}/v1/metrics`, metricsBody));
  }
  await Promise.all(posts);

  // Gate response: see GATE_EVENTS — Trove stays silent (exit 0) so it
  // never blocks the user's session. Populate GATE_EVENTS + emit the
  // verdict shape here if a future rev needs to gate an event.
  if (GATE_EVENTS.has(eventName)) {
    process.stdout.write(JSON.stringify({ permission: 'allow' }) + '\n');
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

// ---------------- Defensive field extraction --------------------------

/** Return the first present non-empty string value among `keys` on
 *  `obj`, or null. Lets the event extractors tolerate field-name drift
 *  across `agy` revs without an interactive re-confirmation. */
function pickString(obj, keys) {
  if (!obj || typeof obj !== 'object') return null;
  for (const k of keys) {
    if (typeof obj[k] === 'string' && obj[k] !== '') return obj[k];
  }
  return null;
}

/** Return the first present finite number among `keys` on `obj`, or
 *  null. Strings that parse as finite numbers are accepted too. */
function pickNumber(obj, keys) {
  if (!obj || typeof obj !== 'object') return null;
  for (const k of keys) {
    const v = obj[k];
    if (typeof v === 'number' && Number.isFinite(v)) return v;
    if (typeof v === 'string' && v !== '' && Number.isFinite(Number(v))) return Number(v);
  }
  return null;
}

// ---------------- Turn correlation (start marker → terminal event) -----

/** Per-host marker directory. We use a single directory under tmpdir so
 *  each (session, slot) pair gets one tiny JSON file holding the turn's
 *  start time and (for chat turns) the prompt byte count. */
const TURN_DIR = path.join(os.tmpdir(), 'trove-antigravity-turns');

function safeId(id) {
  return String(id)
    .replace(/[^A-Za-z0-9_.-]/g, '_')
    .slice(0, 128);
}

/** `slot` separates concurrent turn kinds for one session ("chat" vs
 *  "shell") so an in-flight shell command can't clobber the chat marker. */
function markerPath(sessionId, slot) {
  return path.join(TURN_DIR, `${safeId(sessionId)}__${safeId(slot)}.json`);
}

/** Stash `{ t: startSeconds, pb: promptBytes|null }` for a turn. */
function recordTurnStart(sessionId, slot, nowSeconds, promptBytes) {
  try {
    fs.mkdirSync(TURN_DIR, { recursive: true });
    const body = JSON.stringify({ t: nowSeconds, pb: promptBytes });
    fs.writeFileSync(markerPath(sessionId, slot), body, 'utf8');
  } catch (_err) {
    // Marker write failed — duration + input tokens will be skipped for
    // this turn. The events counter still emits; the rest is best-effort.
  }
}

/** Read + consume the start marker. Returns `{ durationSeconds, promptBytes }`
 *  with either field null when unavailable. */
function takeTurn(sessionId, slot, nowSeconds) {
  const p = markerPath(sessionId, slot);
  let startSeconds = null;
  let promptBytes = null;
  try {
    const raw = fs.readFileSync(p, 'utf8');
    const parsed = JSON.parse(raw);
    if (parsed && typeof parsed === 'object') {
      if (Number.isFinite(parsed.t)) startSeconds = parsed.t;
      if (Number.isFinite(parsed.pb)) promptBytes = parsed.pb;
    } else if (Number.isFinite(Number(raw))) {
      // Tolerate a bare-number marker from an older script rev.
      startSeconds = Number(raw);
    }
  } catch (_err) {
    return { durationSeconds: null, promptBytes: null };
  }
  try {
    fs.unlinkSync(p);
  } catch (_err) {
    // Best-effort cleanup.
  }
  let durationSeconds = null;
  if (startSeconds !== null) {
    const elapsed = nowSeconds - startSeconds;
    if (Number.isFinite(elapsed) && elapsed >= 0 && elapsed <= 3600) {
      durationSeconds = elapsed;
    }
  }
  return { durationSeconds, promptBytes };
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

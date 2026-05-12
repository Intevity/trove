#!/usr/bin/env node
//
// Trove's Cursor hook — vendored, single-file, stdlib-only.
//
// Cursor invokes this script for each registered hook event (currently
// `beforeShellExecution` and `afterShellExecution` per the Sprint 7 MVP).
// We read one JSON event from stdin, transform it into OTLP HTTP/JSON
// payloads (one log record + one metric data point), and POST them to
// the local Trove collector on 127.0.0.1:4318. On any failure we exit 0
// and produce no output — the hook must never block Cursor or surface
// an error to the user.
//
// Why both logs AND metrics: the log carries the structured per-event
// detail (command, exit code, conversation IDs) for raw inspection;
// the metric is a delta counter with `harness.id` + `cursor.event`
// attributes so backend dashboards can plot Cursor activity in their
// metrics views without resorting to log-aggregation queries.
//
// The matching cursor_common.rs adapter writes the absolute path of this
// file into ~/.cursor/hooks.json so Cursor knows to invoke it. The script
// is intentionally idempotent over malformed input: if Cursor changes the
// hook payload shape upstream, the worst case is we drop one event.
//
// To smoke the script outside Cursor, run `cursor-otel-hook.js --health`
// — it exits 0 and writes "ok" to stdout. This is what the adapter's
// integration tests use to verify the bundled file is executable.

'use strict';

const http = require('node:http');
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
  const command = typeof event.command === 'string' ? event.command : null;
  const cwd = typeof event.cwd === 'string' ? event.cwd : null;
  const exitCode = typeof event.exit_code === 'number' ? event.exit_code : null;

  const nowNanos = Date.now() * 1_000_000;

  const logAttributes = [
    stringAttr('cursor.event', eventName),
    stringAttr('cursor.conversation.id', conversationId),
    stringAttr('cursor.generation.id', generationId),
  ];
  if (cursorVersion !== '') logAttributes.push(stringAttr('cursor.version', cursorVersion));
  if (command !== null) logAttributes.push(stringAttr('cursor.shell.command', command));
  if (cwd !== null) logAttributes.push(stringAttr('cursor.shell.cwd', cwd));
  if (exitCode !== null) logAttributes.push(intAttr('cursor.shell.exit_code', exitCode));

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

  // Metric data-point attributes mirror the log's per-event tags so
  // dashboards can slice on `cursor.event` (before/after) and on the
  // shell exit code when present. We keep them tight — high-cardinality
  // values like conversation/generation ids stay on the log signal.
  const metricAttributes = [stringAttr('cursor.event', eventName)];
  if (exitCode !== null) metricAttributes.push(intAttr('cursor.shell.exit_code', exitCode));

  const metricsBody = {
    resourceMetrics: [
      {
        resource: { attributes: resourceAttributes },
        scopeMetrics: [
          {
            scope: { name: 'trove-cursor-hook' },
            metrics: [
              {
                name: 'trove.harness.events',
                unit: '1',
                description:
                  'Count of harness hook events processed by Trove (one per Cursor shell-hook invocation).',
                sum: {
                  // DELTA temporality — the hook is one-shot, so each
                  // invocation contributes exactly one event. isMonotonic
                  // because the counter only ever increases.
                  aggregationTemporality: 1,
                  isMonotonic: true,
                  dataPoints: [
                    {
                      startTimeUnixNano: String(nowNanos),
                      timeUnixNano: String(nowNanos),
                      asInt: '1',
                      attributes: metricAttributes,
                    },
                  ],
                },
              },
            ],
          },
        ],
      },
    ],
  };

  await Promise.all([
    postJson(`${ENDPOINT}/v1/logs`, logsBody),
    postJson(`${ENDPOINT}/v1/metrics`, metricsBody),
  ]);

  // Cursor expects an empty/permissive response on stdout. Echo `allow`
  // for the gate-style events; emit nothing for the post-execution ones.
  if (eventName === 'beforeShellExecution') {
    process.stdout.write(JSON.stringify({ permission: 'allow' }) + '\n');
  }
  process.exit(0);
}

function stringAttr(key, value) {
  return { key, value: { stringValue: String(value) } };
}

function intAttr(key, value) {
  return { key, value: { intValue: String(value) } };
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

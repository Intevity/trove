#!/usr/bin/env node
//
// Trove's Cursor hook — vendored, single-file, stdlib-only.
//
// Cursor invokes this script for each registered hook event (currently
// `beforeShellExecution` and `afterShellExecution` per the Sprint 7 MVP).
// We read one JSON event from stdin, transform it into an OTLP HTTP/JSON
// payload, and POST it to the local Trove collector on 127.0.0.1:4318.
// On any failure we exit 0 and produce no output — the hook must never
// block Cursor or surface an error to the user.
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

  const attributes = [
    stringAttr('cursor.event', eventName),
    stringAttr('cursor.conversation.id', conversationId),
    stringAttr('cursor.generation.id', generationId),
  ];
  if (cursorVersion !== '') attributes.push(stringAttr('cursor.version', cursorVersion));
  if (command !== null) attributes.push(stringAttr('cursor.shell.command', command));
  if (cwd !== null) attributes.push(stringAttr('cursor.shell.cwd', cwd));
  if (exitCode !== null) attributes.push(intAttr('cursor.shell.exit_code', exitCode));

  const resourceAttributes = [
    stringAttr('service.name', 'cursor'),
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
                attributes,
              },
            ],
          },
        ],
      },
    ],
  };

  await postJson(`${ENDPOINT}/v1/logs`, logsBody);

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

import type { CollectorLogLineWire } from '@trove/shared';

export type LogLevel = 'debug' | 'info' | 'warn' | 'error';

export interface ParsedLogLine {
  /** ISO timestamp string captured from the line head, or undefined when
   *  the parser couldn't find one. We keep the raw text so the renderer
   *  can format it with `Date(...)`. */
  timestamp: string | undefined;
  level: LogLevel;
  message: string;
  /** Original unmodified line — used as the source-of-truth for free-text
   *  search filtering so the search hits across timestamp/level/message
   *  regardless of how the parser split them. */
  raw: string;
  stream: string;
}

/** OTel collector text format (Zap encoder, default):
 *
 *  ```
 *  2025-01-15T14:23:01.123-0700\tinfo\tservice@v0.121.0/telemetry.go:103\tSetting up own telemetry...
 *  ```
 *
 *  Some collector builds use spaces instead of tabs and may omit the
 *  timezone. We accept either separator and tolerate a missing timestamp
 *  entirely. Anything we can't classify falls back to `info` (or `warn`
 *  for stderr, since panic traces shouldn't render as info).
 */
const TIMESTAMP_RE = /^(\d{4}-\d{2}-\d{2}[Tt ]\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?)/;
/** Anchored level token: the OTel collector's Zap encoder always emits
 *  level immediately after the timestamp (with a tab or space gap). We
 *  match only at the start of `rest` so a "runtime error" later in a
 *  panic trace doesn't get misclassified as ERROR. */
const LEVEL_RE = /^(debug|info|warn|warning|error|fatal)\b/i;

function normalizeLevel(token: string): LogLevel {
  const t = token.toLowerCase();
  if (t === 'warning') return 'warn';
  if (t === 'fatal') return 'error';
  return t as LogLevel;
}

export function parseCollectorLog(input: CollectorLogLineWire): ParsedLogLine {
  const raw = input.line;
  let rest = raw;
  let timestamp: string | undefined;

  const tsMatch = rest.match(TIMESTAMP_RE);
  if (tsMatch && tsMatch[1] !== undefined) {
    timestamp = tsMatch[1];
    rest = rest.slice(tsMatch[0].length).replace(/^[\t ]+/, '');
  }

  let level: LogLevel | undefined;
  let message = rest;
  const levelMatch = rest.match(LEVEL_RE);
  if (levelMatch && levelMatch[1] !== undefined) {
    level = normalizeLevel(levelMatch[1]);
    const after = rest.slice(levelMatch[0].length).replace(/^[\t ]+/, '');
    message = after.length > 0 ? after : rest;
  }

  if (!level) {
    level = input.stream === 'stderr' ? 'warn' : 'info';
  }

  return { timestamp, level, message, raw, stream: input.stream };
}

/** Format a timestamp string from `ParsedLogLine.timestamp` as `HH:MM:SS.mmm`.
 *  Falls back to empty string when the timestamp is missing or unparseable. */
export function formatLogTime(ts: string | undefined): string {
  if (!ts) return '';
  const d = new Date(ts);
  if (Number.isNaN(d.getTime())) return '';
  const hh = String(d.getHours()).padStart(2, '0');
  const mm = String(d.getMinutes()).padStart(2, '0');
  const ss = String(d.getSeconds()).padStart(2, '0');
  const ms = String(d.getMilliseconds()).padStart(3, '0');
  return `${hh}:${mm}:${ss}.${ms}`;
}

import { describe, expect, it } from 'vitest';

import { formatLogTime, parseCollectorLog } from './parseCollectorLog.js';

describe('parseCollectorLog', () => {
  it('parses an OTel collector tab-separated info line', () => {
    const result = parseCollectorLog({
      stream: 'stdout',
      line: '2025-01-15T14:23:01.123-0700\tinfo\tservice@v0.121.0/telemetry.go:103\tSetting up own telemetry...',
    });
    expect(result.timestamp).toBe('2025-01-15T14:23:01.123-0700');
    expect(result.level).toBe('info');
    expect(result.message).toContain('Setting up own telemetry');
  });

  it('parses a space-separated warn line and folds warning → warn', () => {
    const result = parseCollectorLog({
      stream: 'stdout',
      line: '2025-01-15T14:23:09.044Z warning exporter queue is filling up',
    });
    expect(result.timestamp).toBe('2025-01-15T14:23:09.044Z');
    expect(result.level).toBe('warn');
    expect(result.message).toBe('exporter queue is filling up');
  });

  it('folds fatal into error', () => {
    const result = parseCollectorLog({
      stream: 'stderr',
      line: '2025-01-15T14:23:12.991Z fatal receiver connection refused',
    });
    expect(result.level).toBe('error');
    expect(result.message).toBe('receiver connection refused');
  });

  it('defaults to info for plain stdout text without timestamp or level', () => {
    const result = parseCollectorLog({
      stream: 'stdout',
      line: 'launching collector binary',
    });
    expect(result.timestamp).toBeUndefined();
    expect(result.level).toBe('info');
    expect(result.message).toBe('launching collector binary');
  });

  it('promotes stderr-without-level to warn (panic traces should not look like info)', () => {
    const result = parseCollectorLog({
      stream: 'stderr',
      line: 'panic: runtime error: invalid memory address',
    });
    expect(result.level).toBe('warn');
    expect(result.message).toBe('panic: runtime error: invalid memory address');
  });

  it('ignores level-like words deep inside the message body', () => {
    const result = parseCollectorLog({
      stream: 'stdout',
      // No timestamp; the word "error" appears past the 40-char head — must not promote to error.
      line: 'Connecting to backend... some long preamble before the actual error keyword',
    });
    expect(result.level).toBe('info');
  });

  it('keeps the raw line on the result for free-text search', () => {
    const raw = '2025-01-15T14:23:01.123Z info hello world';
    const result = parseCollectorLog({ stream: 'stdout', line: raw });
    expect(result.raw).toBe(raw);
  });

  it('handles a debug level', () => {
    const result = parseCollectorLog({
      stream: 'stdout',
      line: '2025-01-15T14:23:01.123Z debug pipeline tick',
    });
    expect(result.level).toBe('debug');
  });
});

describe('formatLogTime', () => {
  it('returns HH:MM:SS.mmm in local time', () => {
    // Pin to a UTC time and verify the parts of the formatted output.
    const out = formatLogTime('2025-01-15T14:23:01.456Z');
    expect(out).toMatch(/^\d{2}:\d{2}:\d{2}\.\d{3}$/);
    expect(out.endsWith('.456')).toBe(true);
  });

  it('returns empty string for undefined or unparseable input', () => {
    expect(formatLogTime(undefined)).toBe('');
    expect(formatLogTime('not a date')).toBe('');
  });
});

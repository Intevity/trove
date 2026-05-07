import { describe, expect, it } from 'vitest';

import { IpcCommandName, ListDetectedHarnessesResponse } from './ipc-messages.js';

describe('IpcCommandName', () => {
  it('exposes the list_detected_harnesses Tauri command name', () => {
    expect(IpcCommandName.ListDetectedHarnesses).toBe('list_detected_harnesses');
  });
});

describe('ListDetectedHarnessesResponse', () => {
  it('parses an empty array', () => {
    expect(ListDetectedHarnessesResponse.parse([])).toEqual([]);
  });

  it('parses a one-row response', () => {
    const row = {
      id: 'claude-code',
      detected: true,
      configPath: '/home/me/.claude/settings.json',
      telemetry: 'off',
      detectionMethod: 'config-dir',
    };
    expect(ListDetectedHarnessesResponse.parse([row])).toEqual([row]);
  });

  it('parses a row where the harness is absent', () => {
    const row = {
      id: 'codex-cli',
      detected: false,
      configPath: null,
      telemetry: 'unknown',
      detectionMethod: null,
    };
    expect(ListDetectedHarnessesResponse.parse([row])).toEqual([row]);
  });

  it('rejects a row with an unknown telemetry status', () => {
    expect(() =>
      ListDetectedHarnessesResponse.parse([
        {
          id: 'claude-code',
          detected: true,
          configPath: '/x',
          telemetry: 'partially-on',
          detectionMethod: 'config-dir',
        },
      ]),
    ).toThrow();
  });
});

import { describe, expect, it } from 'vitest';
import { PingRequest, PongResponse } from './ipc-messages.js';

describe('PingRequest', () => {
  it('parses a valid ping', () => {
    expect(PingRequest.parse({ kind: 'ping', nonce: 'abc' })).toEqual({
      kind: 'ping',
      nonce: 'abc',
    });
  });

  it('rejects an empty nonce', () => {
    expect(() => PingRequest.parse({ kind: 'ping', nonce: '' })).toThrow();
  });
});

describe('PongResponse', () => {
  it('parses a valid pong', () => {
    const parsed = PongResponse.parse({
      kind: 'pong',
      nonce: 'abc',
      appVersion: '0.1.0',
    });
    expect(parsed.kind).toBe('pong');
    expect(parsed.appVersion).toBe('0.1.0');
  });
});

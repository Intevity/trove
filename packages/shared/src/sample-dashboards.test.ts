import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

/** Sprint 11 PR 2 — sample dashboards live in `documentation/dashboards/`
 *  for users to import into SigNoz / Grafana / Honeycomb. They are not
 *  loaded by any runtime code path, so this test is the only thing that
 *  catches accidental corruption (a stray comma, a missing brace).
 *
 *  We assert two things per file:
 *  1. The JSON parses.
 *  2. The shape carries the keys the destination backend's importer
 *     requires. The exact panel content is reviewed manually against a
 *     live backend before merge — the test exists so that a future
 *     drive-by edit doesn't silently break the import contract. */
const HERE = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(HERE, '..', '..', '..');

function readDashboard(name: string): unknown {
  return JSON.parse(readFileSync(resolve(REPO_ROOT, 'documentation', 'dashboards', name), 'utf8'));
}

describe('sample dashboards', () => {
  it('grafana dashboard parses and has required top-level keys', () => {
    const d = readDashboard('grafana-trove.json') as Record<string, unknown>;
    expect(d.title).toBe('Trove — AI Coding Telemetry');
    expect(typeof d.schemaVersion).toBe('number');
    expect(Array.isArray(d.panels)).toBe(true);
    expect((d.panels as unknown[]).length).toBe(5);
    expect(Array.isArray(d.tags)).toBe(true);
    expect(d.templating).toBeDefined();
    expect(d.time).toBeDefined();
  });

  it('signoz dashboard parses and has required top-level keys', () => {
    const d = readDashboard('signoz-trove.json') as Record<string, unknown>;
    expect(d.title).toBe('Trove — AI Coding Telemetry');
    expect(Array.isArray(d.tags)).toBe(true);
    expect(Array.isArray(d.widgets)).toBe(true);
    expect((d.widgets as unknown[]).length).toBe(5);
    expect(d.layout).toBeDefined();
    expect(d.variables).toBeDefined();
  });

  it('honeycomb dashboard parses and has required top-level keys', () => {
    const d = readDashboard('honeycomb-trove.json') as Record<string, unknown>;
    expect(d.name).toBe('Trove — AI Coding Telemetry');
    expect(typeof d.description).toBe('string');
    expect(Array.isArray(d.queries)).toBe(true);
    expect((d.queries as unknown[]).length).toBe(5);
  });

  it('every dashboard groups by trove.source somewhere', () => {
    for (const name of ['grafana-trove.json', 'signoz-trove.json', 'honeycomb-trove.json']) {
      const raw = readFileSync(resolve(REPO_ROOT, 'documentation', 'dashboards', name), 'utf8');
      // Plan calls for every panel to break down on this attribute. The
      // exact JSON shape varies per backend; checking the raw text is the
      // simplest cross-format way to confirm the contract isn't lost.
      expect(raw).toMatch(/trove[._]source/);
    }
  });
});

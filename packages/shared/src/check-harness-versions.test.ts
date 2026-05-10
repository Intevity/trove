import { describe, expect, it } from 'vitest';

import {
  checkHarnessVersions,
  compareSemver,
  renderReport,
  type Fetcher,
  type Manifest,
} from '../../../scripts/check-harness-versions.js';

/** Manifest fixture used by every test in this file. */
function manifest(extra?: Partial<Manifest['harnesses']>): Manifest {
  return {
    harnesses: {
      'claude-code': {
        source: 'npm',
        package: '@anthropic-ai/claude-code',
        pinnedVersion: '2.0.0',
      },
      aider: { source: 'pypi', package: 'aider-chat', pinnedVersion: '0.85.0' },
      'cursor-ide': {
        source: 'github',
        package: 'getcursor/cursor',
        pinnedVersion: '0.45.0',
        active: false,
      },
      ...extra,
    },
  };
}

describe('compareSemver', () => {
  it('returns >0 when first is newer', () => {
    expect(compareSemver('2.1.0', '2.0.0')).toBeGreaterThan(0);
    expect(compareSemver('1.0.1', '1.0.0')).toBeGreaterThan(0);
  });
  it('returns <0 when first is older', () => {
    expect(compareSemver('1.9.9', '2.0.0')).toBeLessThan(0);
  });
  it('returns 0 when equal', () => {
    expect(compareSemver('1.2.3', '1.2.3')).toBe(0);
  });
  it('strips leading v', () => {
    expect(compareSemver('v1.2.3', '1.2.3')).toBe(0);
  });
  it('ignores pre-release suffixes', () => {
    expect(compareSemver('1.2.3-rc1', '1.2.3')).toBe(0);
  });
  it('handles different segment counts', () => {
    expect(compareSemver('1.2', '1.2.0')).toBe(0);
    expect(compareSemver('1', '1.0.1')).toBeLessThan(0);
  });
  it('falls back to 0 for malformed segments', () => {
    expect(compareSemver('abc', 'def')).toBe(0);
  });
});

describe('checkHarnessVersions', () => {
  it('reports no drifts when all pinned == latest', async () => {
    const fetcher: Fetcher = async (e) => e.pinnedVersion;
    const result = await checkHarnessVersions(manifest(), fetcher);
    expect(result.drifts).toEqual([]);
    expect(result.errors).toEqual([]);
    expect(result.skipped).toEqual(['cursor-ide']);
  });

  it('reports drift when latest is newer', async () => {
    const fetcher: Fetcher = async (e) => {
      if (e.package === 'aider-chat') return '0.86.0';
      return e.pinnedVersion;
    };
    const result = await checkHarnessVersions(manifest(), fetcher);
    expect(result.drifts).toHaveLength(1);
    expect(result.drifts[0]).toMatchObject({
      id: 'aider',
      pinnedVersion: '0.85.0',
      latestVersion: '0.86.0',
    });
  });

  it('skips inactive entries even when newer is published', async () => {
    const fetcher: Fetcher = async (e) => (e.source === 'github' ? '99.0.0' : e.pinnedVersion);
    const result = await checkHarnessVersions(manifest(), fetcher);
    expect(result.drifts).toEqual([]);
    expect(result.skipped).toContain('cursor-ide');
  });

  it('captures fetch errors per-entry instead of bailing out', async () => {
    const fetcher: Fetcher = async (e) => {
      if (e.package === 'aider-chat') throw new Error('PyPI 503');
      return e.pinnedVersion;
    };
    const result = await checkHarnessVersions(manifest(), fetcher);
    expect(result.drifts).toEqual([]);
    expect(result.errors).toEqual([{ id: 'aider', reason: 'PyPI 503' }]);
  });

  it('coerces non-Error throws to string', async () => {
    const fetcher: Fetcher = async () => {
      throw 'kaboom';
    };
    const result = await checkHarnessVersions(manifest(), fetcher);
    expect(result.errors[0]?.reason).toBe('kaboom');
  });

  it('treats older latest than pinned as no drift (downgrade safety)', async () => {
    const fetcher: Fetcher = async () => '0.0.1';
    const result = await checkHarnessVersions(manifest(), fetcher);
    expect(result.drifts).toEqual([]);
  });
});

describe('renderReport', () => {
  it('renders a drift table with the run URL when provided', () => {
    const md = renderReport(
      {
        drifts: [
          {
            id: 'claude-code',
            source: 'npm',
            package: '@anthropic-ai/claude-code',
            pinnedVersion: '2.0.0',
            latestVersion: '2.1.0',
          },
        ],
        errors: [],
        skipped: [],
      },
      'https://github.com/Intevity/trove/actions/runs/42',
    );
    expect(md).toContain('Adapter version drift');
    expect(md).toContain('| Harness |');
    expect(md).toContain('claude-code');
    expect(md).toContain('2.1.0');
    expect(md).toContain('runs/42');
  });

  it('renders fetch errors when present', () => {
    const md = renderReport(
      { drifts: [], errors: [{ id: 'aider', reason: 'PyPI 503' }], skipped: [] },
      null,
    );
    expect(md).toContain('Fetch errors');
    expect(md).toContain('aider');
    expect(md).toContain('PyPI 503');
  });

  it('renders inactive entries when present', () => {
    const md = renderReport({ drifts: [], errors: [], skipped: ['cursor-ide', 'cline'] }, null);
    expect(md).toContain('Inactive entries');
    expect(md).toContain('cursor-ide');
    expect(md).toContain('cline');
  });

  it('omits the run-URL line when the env vars are missing', () => {
    const md = renderReport({ drifts: [], errors: [], skipped: [] }, null);
    expect(md).not.toContain('Triggered by');
  });
});

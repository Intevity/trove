/**
 * Sprint 11 PR 3 — version-drift detector for the harness adapters.
 *
 * The nightly workflow runs this script. For every active entry in
 * `resources/harness-versions.json` it fetches the current latest
 * published version from npm / PyPI / GitHub Releases and compares it
 * to `pinnedVersion`. Any drift is printed and the process exits
 * non-zero so the workflow can open a GitHub issue.
 *
 * The pinned version is the version Trove's adapter was last validated
 * against. Bumping it is the manual maintainer action after re-running
 * the golden-file suite for that harness against the new upstream.
 *
 * Pure check + IO are split so the function can be unit-tested with a
 * stub fetcher.
 */

import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export type HarnessSource = 'npm' | 'pypi' | 'github';

export interface ManifestEntry {
  source: HarnessSource;
  package: string;
  pinnedVersion: string;
  active?: boolean;
}

export interface Manifest {
  harnesses: Record<string, ManifestEntry>;
}

export interface DriftEntry {
  id: string;
  source: HarnessSource;
  package: string;
  pinnedVersion: string;
  latestVersion: string;
}

export interface CheckResult {
  drifts: DriftEntry[];
  errors: { id: string; reason: string }[];
  skipped: string[];
}

export type Fetcher = (entry: ManifestEntry) => Promise<string>;

/** Pure check function — exported for testing. Takes the manifest and a
 *  fetcher (HTTP client wrapper) and returns the diff. The CLI wraps it
 *  with the real fetcher and an exit-code policy. */
export async function checkHarnessVersions(
  manifest: Manifest,
  fetcher: Fetcher,
): Promise<CheckResult> {
  const drifts: DriftEntry[] = [];
  const errors: { id: string; reason: string }[] = [];
  const skipped: string[] = [];

  for (const [id, entry] of Object.entries(manifest.harnesses)) {
    if (entry.active === false) {
      skipped.push(id);
      continue;
    }
    try {
      const latest = await fetcher(entry);
      if (compareSemver(latest, entry.pinnedVersion) > 0) {
        drifts.push({
          id,
          source: entry.source,
          package: entry.package,
          pinnedVersion: entry.pinnedVersion,
          latestVersion: latest,
        });
      }
    } catch (err) {
      errors.push({ id, reason: err instanceof Error ? err.message : String(err) });
    }
  }

  return { drifts, errors, skipped };
}

/** Best-effort semver comparison. Returns >0 if `a` is newer than `b`,
 *  0 if equal, <0 if older. Tolerates leading 'v' and pre-release
 *  suffixes (which it ignores rather than failing). */
export function compareSemver(a: string, b: string): number {
  const norm = (v: string): number[] => {
    const cleaned = v.replace(/^v/, '').split(/[-+]/)[0] ?? '0';
    return cleaned.split('.').map((n) => Number.parseInt(n, 10) || 0);
  };
  const av = norm(a);
  const bv = norm(b);
  const len = Math.max(av.length, bv.length);
  for (let i = 0; i < len; i++) {
    const diff = (av[i] ?? 0) - (bv[i] ?? 0);
    if (diff !== 0) return diff;
  }
  return 0;
}

/** Real fetcher used by the CLI. Each source has its own latest-version
 *  endpoint shape. */
export const realFetcher: Fetcher = async (entry) => {
  switch (entry.source) {
    case 'npm':
      return fetchNpmLatest(entry.package);
    case 'pypi':
      return fetchPypiLatest(entry.package);
    case 'github':
      return fetchGithubLatest(entry.package);
  }
};

async function fetchJson(url: string, headers?: Record<string, string>): Promise<unknown> {
  const r = await fetch(url, { headers });
  if (!r.ok) throw new Error(`${url} -> HTTP ${r.status}`);
  return r.json();
}

async function fetchNpmLatest(pkg: string): Promise<string> {
  const url = `https://registry.npmjs.org/${pkg}/latest`;
  const body = (await fetchJson(url)) as { version?: string };
  if (!body.version) throw new Error(`npm: ${pkg} returned no version field`);
  return body.version;
}

async function fetchPypiLatest(pkg: string): Promise<string> {
  const url = `https://pypi.org/pypi/${encodeURIComponent(pkg)}/json`;
  const body = (await fetchJson(url)) as { info?: { version?: string } };
  if (!body.info?.version) throw new Error(`pypi: ${pkg} returned no info.version`);
  return body.info.version;
}

async function fetchGithubLatest(repo: string): Promise<string> {
  const url = `https://api.github.com/repos/${repo}/releases/latest`;
  const headers: Record<string, string> = { Accept: 'application/vnd.github+json' };
  const token = process.env.GITHUB_TOKEN;
  if (token) headers.Authorization = `Bearer ${token}`;
  const body = (await fetchJson(url, headers)) as { tag_name?: string };
  if (!body.tag_name) throw new Error(`github: ${repo} returned no tag_name`);
  return body.tag_name;
}

/** Render the result as the Markdown body for an `adapter-regression`
 *  GitHub issue. Returned as a string so the workflow can write it to
 *  a file and pass it to `gh issue create --body-file`. */
export function renderReport(result: CheckResult, runUrl: string | null): string {
  const lines: string[] = [];
  lines.push('# Adapter version drift detected');
  lines.push('');
  if (runUrl) {
    lines.push(`Triggered by [nightly run](${runUrl}).`);
    lines.push('');
  }
  if (result.drifts.length > 0) {
    lines.push('## Newer upstream versions');
    lines.push('');
    lines.push('| Harness | Source | Package | Pinned | Latest |');
    lines.push('|---|---|---|---|---|');
    for (const d of result.drifts) {
      lines.push(
        `| \`${d.id}\` | ${d.source} | \`${d.package}\` | ${d.pinnedVersion} | ${d.latestVersion} |`,
      );
    }
    lines.push('');
    lines.push(
      'Action: re-run the adapter golden-file suite for each row against the new upstream. If still passing, bump `pinnedVersion` in `resources/harness-versions.json` to the latest. If failing, open a follow-up to update the adapter.',
    );
    lines.push('');
  }
  if (result.errors.length > 0) {
    lines.push('## Fetch errors');
    lines.push('');
    for (const e of result.errors) {
      lines.push(`- \`${e.id}\`: ${e.reason}`);
    }
    lines.push('');
  }
  if (result.skipped.length > 0) {
    lines.push(
      `## Inactive entries (skipped)\n\n${result.skipped.map((s) => `- \`${s}\``).join('\n')}\n`,
    );
  }
  return lines.join('\n');
}

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(HERE, '..');

async function main(): Promise<void> {
  const manifestPath = resolve(REPO_ROOT, 'resources', 'harness-versions.json');
  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8')) as Manifest;
  const result = await checkHarnessVersions(manifest, realFetcher);

  const runUrl =
    process.env.GITHUB_SERVER_URL && process.env.GITHUB_REPOSITORY && process.env.GITHUB_RUN_ID
      ? `${process.env.GITHUB_SERVER_URL}/${process.env.GITHUB_REPOSITORY}/actions/runs/${process.env.GITHUB_RUN_ID}`
      : null;

  const report = renderReport(result, runUrl);
  process.stdout.write(report);
  process.stdout.write('\n');

  if (result.drifts.length > 0 || result.errors.length > 0) {
    process.exitCode = 1;
  }
}

const invokedAsScript =
  import.meta.url === `file://${process.argv[1]}` ||
  process.argv[1]?.endsWith('check-harness-versions.ts');
if (invokedAsScript) {
  void main();
}

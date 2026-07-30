#!/usr/bin/env node
// Assemble the Tauri updater manifest (latest.json) from a directory of
// downloaded release artifacts. Runs in notarize-finalize.yml's
// publish-manifest job after the macOS tarballs have been stapled +
// re-signed, so the signatures read here are the final ones.
//
// Usage:
//   UPDATER_ARTIFACT_BASE=https://github.com/OWNER/REPO/releases/download/vX.Y.Z \
//     node scripts/assemble-latest-json.mjs <dir> <version>
//
// <dir> holds the updater artifacts + their minisign .sig companions
// (downloaded from the draft release); <version> is the bare semver
// (tag minus the leading v). Writes <dir>/latest.json and prints it.
//
// Env:
//   UPDATER_ARTIFACT_BASE  base URL the artifacts are served from — the
//                          release's own download prefix, since the GitHub
//                          release IS the update channel (installed apps
//                          poll releases/latest/download/latest.json)
//
// Exit codes:
//   0  manifest written, all expected platforms present
//   1  unmapped artifact, missing .sig, duplicate key, or missing platform
//
// Platform keys: tauri-plugin-updater looks up {target}-{arch}-{bundle}
// first, then falls back to {target}-{arch}. We emit both, mirroring
// tauri-action's scheme — with one deliberate divergence: our bare
// windows-x86_64 points at the NSIS -setup.exe (tauri-action's points at
// the MSI). NSIS is Tauri's recommended updater installer (passive
// in-place reinstall, per-user installs without elevation); the MSI stays
// reachable under windows-x86_64-msi. Do not "fix" this back to MSI.

import { readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

function fail(msg) {
  console.error(`::error::${msg}`);
  process.exit(1);
}

// Map an artifact filename to its updater platform keys: the
// bundle-suffixed key plus (for the bundle that bare lookups should get)
// the bare {target}-{arch} fallback. Every non-macOS leg is x86_64, and the
// single macOS leg ships a universal2 bundle, so the bundle extension pins the
// OS and only the macOS tarball needs an arch sniff.
function platformKeys(name) {
  if (name.endsWith('.app.tar.gz')) {
    // Tauri names the macOS updater tarballs <product>_aarch64.app.tar.gz,
    // <product>_x64.app.tar.gz (x64, NOT x86_64), or — for a universal2 build —
    // <product>_universal.app.tar.gz. A universal tarball runs natively on BOTH
    // arches, so it answers all four darwin keys (each existing arm64/Intel
    // install resolves its own key to this one file); per-arch tarballs map to
    // just their own. REQUIRED_KEYS is unchanged — one universal file satisfies
    // both darwin entries, and the duplicate-key guard below still fires if a
    // universal AND a per-arch tarball ever land in the same release.
    if (name.includes('universal'))
      return ['darwin-aarch64-app', 'darwin-aarch64', 'darwin-x86_64-app', 'darwin-x86_64'];
    if (name.includes('aarch64')) return ['darwin-aarch64-app', 'darwin-aarch64'];
    if (name.includes('x64') || name.includes('x86_64'))
      return ['darwin-x86_64-app', 'darwin-x86_64'];
    fail(`cannot determine macOS arch from ${name}`);
  }
  if (name.endsWith('.AppImage')) return ['linux-x86_64-appimage', 'linux-x86_64'];
  if (name.endsWith('.deb')) return ['linux-x86_64-deb'];
  if (name.endsWith('.rpm')) return ['linux-x86_64-rpm'];
  if (name.endsWith('-setup.exe')) return ['windows-x86_64-nsis', 'windows-x86_64'];
  if (name.endsWith('.msi')) return ['windows-x86_64-msi'];
  return null;
}

// Every release builds every bundle type (bundle.targets "all" on each
// matrix leg), so a missing key here means a build or upload failure that
// must surface — never publish a manifest silently missing a platform.
const REQUIRED_KEYS = [
  'darwin-aarch64-app',
  'darwin-x86_64-app',
  'linux-x86_64-appimage',
  'linux-x86_64-deb',
  'linux-x86_64-rpm',
  'windows-x86_64-nsis',
  'windows-x86_64-msi',
];

// Name-only completeness check (used by the scheduled notarize-poll workflow).
// Verify a draft release carries a COMPLETE updater artifact set WITHOUT
// downloading anything, so the poller can refuse to finalize — and never spin up
// the 10x macOS staple job — for a release that can never produce a manifest
// (e.g. a timed-out build missing its Linux bundles). Reuses platformKeys() +
// REQUIRED_KEYS + the .sig-companion rule, so this can never drift from the real
// (content-mode) assemble below.
//
//   node scripts/assemble-latest-json.mjs --check-names <file-of-asset-names>
//
// <file-of-asset-names> holds the draft's asset names, one per line (e.g. from
// `gh release view <tag> --json assets --jq '.assets[].name'`). Non-updater
// assets (.dmg, .sig, latest.json, notary-*.json) map to no key and are skipped.
//
// Exit codes:
//   0  every REQUIRED_KEYS platform present (with its .sig) -> safe to finalize
//   1  a required platform or a .sig companion is missing   -> never finalizes
if (process.argv[2] === '--check-names') {
  const namesFile = process.argv[3];
  if (!namesFile) fail('usage: assemble-latest-json.mjs --check-names <file-of-asset-names>');
  const names = readFileSync(namesFile, 'utf8')
    .split('\n')
    .map((s) => s.trim())
    .filter(Boolean);
  const have = new Set(names);
  const present = {};
  for (const name of names) {
    const keys = platformKeys(name);
    if (!keys) continue; // not an updater artifact (.dmg/.sig/latest.json/notary-*.json)
    if (!have.has(`${name}.sig`)) fail(`missing signature companion ${name}.sig`);
    for (const key of keys) present[key] = name;
  }
  const missing = REQUIRED_KEYS.filter((k) => !present[k]);
  if (missing.length) fail(`release is missing updater artifacts for: ${missing.join(', ')}`);
  console.log('Release artifact set is complete for all required platforms.');
  process.exit(0);
}

const [dir, version] = process.argv.slice(2);
if (!dir || !version) fail('usage: assemble-latest-json.mjs <dir> <version>');

const base = (process.env.UPDATER_ARTIFACT_BASE || '').replace(/\/+$/, '');
if (!base) fail('UPDATER_ARTIFACT_BASE is not set');

const platforms = {};
for (const f of readdirSync(dir).sort()) {
  // .sig files are consumed as companions of the artifact they sign; a
  // pre-existing latest.json just means this dir has been assembled before.
  if (f.endsWith('.sig') || f === 'latest.json') continue;
  const keys = platformKeys(f);
  if (!keys) fail(`unmapped artifact ${f} — extend platformKeys() or fix the download globs`);
  let signature;
  try {
    signature = readFileSync(join(dir, `${f}.sig`), 'utf8').trim();
  } catch {
    fail(`missing signature companion ${f}.sig`);
  }
  const entry = { signature, url: `${base}/${encodeURIComponent(f)}` };
  for (const key of keys) {
    if (platforms[key]) fail(`both ${platforms[key].url} and ${f} map to ${key}`);
    platforms[key] = entry;
  }
}

const missing = REQUIRED_KEYS.filter((k) => !platforms[k]);
if (missing.length) fail(`release is missing updater artifacts for: ${missing.join(', ')}`);

const manifest = {
  version,
  notes: `Trove ${version}`,
  pub_date: new Date().toISOString(),
  platforms: Object.fromEntries(Object.entries(platforms).sort(([a], [b]) => a.localeCompare(b))),
};
writeFileSync(join(dir, 'latest.json'), JSON.stringify(manifest, null, 2));
console.log(JSON.stringify(manifest, null, 2));

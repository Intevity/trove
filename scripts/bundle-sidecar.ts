/**
 * Stages the per-platform `trove-otelcol` binary produced by
 * `pnpm build:collector` into `packages/app/src-tauri/binaries/` with the
 * platform-triple suffix Tauri's `externalBin` expects.
 *
 * Tauri's bundler reads `externalBin: ["binaries/trove-otelcol"]` from
 * `tauri.conf.json` and resolves it at build time to the file with the
 * matching `<rust-target-triple>` suffix in the binaries/ directory.
 * It then strips the suffix when packaging the final app bundle, so the
 * Rust supervisor sees a plain `trove-otelcol[.exe]` next to the app
 * executable at runtime.
 *
 * Triple resolution mirrors scripts/build-collector.sh — explicit override
 * via TROVE_TARGET_TRIPLE wins, then CARGO_BUILD_TARGET / TAURI_ENV_TARGET_TRIPLE,
 * then `rustc -vV` host detection.
 *
 * This file is loaded via `jiti` from the root package.json
 * `bundle:sidecar` script.
 */

import { execSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(__dirname, '..');
const DIST_ROOT = join(ROOT, 'resources', 'otelcol', 'dist');
const BINARIES_DIR = join(ROOT, 'packages', 'app', 'src-tauri', 'binaries');

function resolveTriple(): string {
  const fromEnv =
    process.env['TROVE_TARGET_TRIPLE'] ||
    process.env['CARGO_BUILD_TARGET'] ||
    process.env['TAURI_ENV_TARGET_TRIPLE'];
  if (fromEnv) return fromEnv;

  try {
    const out = execSync('rustc -vV', { encoding: 'utf8' });
    const match = out.match(/^host:\s*(.+)$/m);
    if (!match || !match[1]) {
      throw new Error('rustc -vV did not include a host: line');
    }
    return match[1].trim();
  } catch (err) {
    const reason = err instanceof Error ? err.message : String(err);
    throw new Error(
      `Could not detect host target triple from rustc. ` +
        `Set TROVE_TARGET_TRIPLE or install Rust. (${reason})`,
    );
  }
}

function main(): void {
  const triple = resolveTriple();
  const ext = triple.includes('windows') ? '.exe' : '';
  const sourceBin = join(DIST_ROOT, triple, `trove-otelcol${ext}`);
  const targetBin = join(BINARIES_DIR, `trove-otelcol-${triple}${ext}`);

  if (!existsSync(sourceBin)) {
    throw new Error(
      `Source binary missing: ${sourceBin}\n` +
        `Run \`pnpm build:collector\` first (or set TROVE_TARGET_TRIPLE if cross-bundling).`,
    );
  }

  mkdirSync(BINARIES_DIR, { recursive: true });
  copyFileSync(sourceBin, targetBin);

  const sizeMb = (statSync(targetBin).size / 1024 / 1024).toFixed(1);
  console.log(`[bundle-sidecar] triple : ${triple}`);
  console.log(`[bundle-sidecar] from   : ${sourceBin}`);
  console.log(`[bundle-sidecar] to     : ${targetBin} (${sizeMb} MB)`);
}

main();

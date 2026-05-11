import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { execSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

const __dirname = dirname(fileURLToPath(import.meta.url));

// Resolve the version string shown in the footer.
//   1. Tagged CI build: GitHub Actions sets GITHUB_REF_TYPE=tag and
//      GITHUB_REF_NAME to the tag (e.g. "v0.5.0").
//   2. Tagged local build: `git describe --tags --exact-match` returns the tag.
//   3. Untagged local dev: fall back to "dev".
function getAppVersion(): string {
  if (process.env.GITHUB_REF_TYPE === 'tag' && process.env.GITHUB_REF_NAME) {
    return process.env.GITHUB_REF_NAME;
  }
  try {
    const tag = execSync('git describe --tags --exact-match HEAD', {
      stdio: ['ignore', 'pipe', 'ignore'],
    })
      .toString()
      .trim();
    if (tag) return tag;
  } catch {
    // HEAD not on a tag — fall through.
  }
  return 'dev';
}

// Tauri uses port 1420 by default; align with that and fail loudly if taken.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  define: {
    __APP_VERSION__: JSON.stringify(getAppVersion()),
  },
  server: {
    port: 1420,
    strictPort: true,
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    target: 'es2022',
  },
  resolve: {
    alias: {
      '@trove/shared': resolve(__dirname, '../shared/src/index.ts'),
    },
  },
});

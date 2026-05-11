import { defineConfig } from 'vitest/config';

// Sprint 2 ratchet: the loose 60/60/0/0 thresholds from Sprint 0 are
// replaced with the 95/95/95/93 set claude-sentinel uses. Rust coverage
// (the bulk of Sprint 2's new code) is gated separately by
// `cargo llvm-cov --fail-under-lines 95 --fail-under-functions 95` in CI.
export default defineConfig({
  // The app's Vite config injects __APP_VERSION__ at build time; mirror that
  // here so test runs of the Footer / bugReport util don't crash on the
  // dangling global.
  define: {
    __APP_VERSION__: JSON.stringify('dev'),
  },
  test: {
    globals: true,
    environment: 'node',
    include: [
      'packages/*/src/**/*.test.ts',
      'packages/*/src/**/*.test.tsx',
      'packages/*/src/**/*.spec.ts',
    ],
    exclude: ['**/node_modules/**', '**/dist/**', 'packages/app/e2e/**'],
    environmentMatchGlobs: [['packages/app/src/**', 'jsdom']],
    coverage: {
      provider: 'v8',
      reporter: ['text', 'json', 'html'],
      include: ['packages/*/src/**/*.ts', 'packages/*/src/**/*.tsx'],
      exclude: [
        'node_modules/**',
        '**/dist/**',
        '**/*.d.ts',
        '**/*.test.ts',
        '**/*.test.tsx',
        '**/*.spec.ts',
        'vitest.config.ts',
        'eslint.config.ts',
        // App frontend is exercised end-to-end via Playwright (Sprint 6 will
        // expand the surface). Unit-coverage gating sits on packages/shared.
        'packages/app/src/**',
        // Pure re-export barrels carry no logic; v8 reports them as
        // 0% covered which sinks the aggregate even when the actual
        // schema and IPC files sit at 100%.
        'packages/shared/src/index.ts',
      ],
      thresholds: {
        lines: 95,
        statements: 95,
        functions: 95,
        // Branches at 93 mirrors claude-sentinel's gate; some Zod
        // fallbacks (e.g. `.default(...)` shapes) introduce branches the
        // tests don't all hit, and pinning to 93 leaves headroom for
        // upcoming schema additions without paper-cut threshold drift.
        branches: 93,
      },
    },
  },
});

import { defineConfig } from 'vitest/config';

// Sprint 0 thresholds are intentionally loose (60%). They will ratchet to 95%
// in Sprint 2 when the safety toolkit lands and there is real code worth
// gating on.
export default defineConfig({
  test: {
    globals: true,
    environment: 'node',
    // Sprint 0 PR #1 has no test files yet (PR #2 adds packages/{shared,app}).
    // Don't fail the run when no tests match.
    passWithNoTests: true,
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
      ],
      // Sprint 0 has only declarative Zod schemas in packages/shared — zero
      // traditional functions or conditional branches to cover. Lines and
      // statements stay at 60; functions and branches relax to 0 until
      // Sprint 1+ introduces real Rust + TS function code worth gating on.
      // All four ratchet to 95 in Sprint 2 alongside the safety toolkit.
      thresholds: {
        lines: 60,
        statements: 60,
        functions: 0,
        branches: 0,
      },
    },
  },
});

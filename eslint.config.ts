import eslint from '@eslint/js';
import tseslint from 'typescript-eslint';
import reactHooks from 'eslint-plugin-react-hooks';

export default tseslint.config(
  eslint.configs.recommended,
  ...tseslint.configs.recommended,
  {
    ignores: [
      '**/dist/**',
      '**/node_modules/**',
      '**/coverage/**',
      '**/*.d.ts',
      'packages/app/src-tauri/**',
      'packages/app/e2e/**',
      // Vendored hook / plugin scripts shipped via Tauri's bundle.resources.
      // Each runs under its own runtime model (Cursor invokes the .cjs hook
      // directly via Node; the OpenCode plugin in Sprint 7 PR 2 will be
      // bundled by OpenCode). Linting them with the app's TS config produces
      // false positives (require(), CommonJS catch params, etc.).
      'resources/**',
      // `.claude/worktrees/<name>/` are isolated git worktrees Plumb/Claude
      // Code creates for ephemeral sessions. Already gitignored, but ESLint
      // doesn't honor `.gitignore`, so we exclude them here too — otherwise
      // their vendored `resources/**` files re-trigger CommonJS lint errors.
      '**/.claude/**',
    ],
  },
  {
    plugins: { 'react-hooks': reactHooks },
    rules: { 'react-hooks/exhaustive-deps': 'warn' },
  },
  {
    languageOptions: {
      globals: {
        console: 'readonly',
        process: 'readonly',
        Buffer: 'readonly',
        setTimeout: 'readonly',
        clearTimeout: 'readonly',
        setInterval: 'readonly',
        clearInterval: 'readonly',
        setImmediate: 'readonly',
        clearImmediate: 'readonly',
        module: 'readonly',
        require: 'readonly',
        __dirname: 'readonly',
        __filename: 'readonly',
        global: 'readonly',
        globalThis: 'readonly',
      },
    },
    rules: {
      '@typescript-eslint/no-explicit-any': 'error',
      '@typescript-eslint/explicit-function-return-type': 'off',
      '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_' }],
      'no-console': 'off',
    },
  },
);

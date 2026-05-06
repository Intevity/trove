import { HarnessId } from '@trove/shared';

export function App(): JSX.Element {
  const harnessCount = HarnessId.options.length;

  return (
    <main className="min-h-screen bg-slate-50 text-slate-900 antialiased dark:bg-slate-950 dark:text-slate-100">
      <div className="mx-auto flex min-h-screen max-w-2xl flex-col items-start justify-center gap-4 px-8 py-12">
        <h1 data-testid="app-header" className="text-3xl font-semibold tracking-tight">
          Hello, Trove
        </h1>
        <p className="text-base text-slate-600 dark:text-slate-400">
          A vendor-neutral configurator and OTLP gateway for AI coding harnesses. Detection and
          patching arrive in Sprint 3.
        </p>
        <p className="text-sm text-slate-500 dark:text-slate-500">
          Targeting <span className="font-mono">{harnessCount}</span> harnesses for the MVP.
        </p>
      </div>
    </main>
  );
}

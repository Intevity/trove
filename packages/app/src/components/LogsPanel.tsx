import { useCollectorLogTail } from '../hooks/useCollectorLogTail.js';

/** Live tail of `collector.log`. Initial render fetches the last 200
 *  lines via IPC; subsequent lines arrive on the `collector-log`
 *  Tauri event. Bounded ring buffer (5000 lines) lives inside the
 *  `useCollectorLogTail` hook. */
export function LogsPanel(): JSX.Element {
  const { lines, loading } = useCollectorLogTail();

  return (
    <section
      data-testid="logs-panel"
      className="flex flex-col flex-1 min-h-0 rounded-md border border-slate-200 bg-white px-4 py-3 dark:border-slate-800 dark:bg-slate-900"
    >
      <header className="mb-2 flex flex-shrink-0 items-center justify-between">
        <h2 className="text-sm font-semibold text-slate-900 dark:text-slate-100">Logs</h2>
        <span className="text-xs text-slate-500 dark:text-slate-400">
          {loading ? 'Loading…' : `${lines.length} lines`}
        </span>
      </header>
      <pre
        data-testid="logs-output"
        className="flex-1 min-h-0 overflow-auto whitespace-pre-wrap break-all rounded bg-slate-50 px-2 py-1 font-mono text-xs leading-tight text-slate-800 dark:bg-slate-950 dark:text-slate-200"
      >
        {lines.length === 0
          ? loading
            ? 'Loading…'
            : '(no log output yet)'
          : lines.map((line, idx) => (
              <div key={idx} data-stream={line.stream}>
                {line.line}
              </div>
            ))}
      </pre>
    </section>
  );
}

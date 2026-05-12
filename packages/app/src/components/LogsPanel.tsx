import { ArrowDown, Search, X } from 'lucide-react';
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';

import { useCollectorLogTail } from '../hooks/useCollectorLogTail.js';
import {
  type LogLevel,
  type ParsedLogLine,
  formatLogTime,
  parseCollectorLog,
} from '../lib/parseCollectorLog.js';

/** Rendering 5000 rows in a non-virtualized list is laggy. Cap the visible
 *  slice and offer a "Show all" escape hatch for the rare power-user
 *  case. Mirrors claude-sentinel/.../LogsViewer.tsx VISIBLE_CAP. */
const VISIBLE_CAP = 500;

const LEVEL_LABEL: Record<LogLevel, string> = {
  debug: 'DEBUG',
  info: 'INFO',
  warn: 'WARN',
  error: 'ERROR',
};

const LEVEL_STYLE: Record<LogLevel, string> = {
  debug: 'text-slate-400 dark:text-slate-500',
  info: 'text-slate-700 dark:text-slate-200',
  warn: 'text-ios-orange',
  error: 'text-ios-red',
};

/** Live tail of `collector.log`. Initial render fetches the last 200
 *  lines via IPC; subsequent lines arrive on the `collector-log`
 *  Tauri event. Bounded ring buffer (5000 lines) lives inside the
 *  `useCollectorLogTail` hook.
 *
 *  Each line is parsed by `parseCollectorLog` into timestamp / level /
 *  message so we can color-code by severity. The parser is lenient —
 *  lines it can't classify render as `info` (or `warn` for stderr). */
export function LogsPanel(): JSX.Element {
  const { lines, loading } = useCollectorLogTail();

  const [query, setQuery] = useState('');
  const [debouncedQuery, setDebouncedQuery] = useState('');
  const [showAll, setShowAll] = useState(false);

  useEffect(() => {
    const id = setTimeout(() => setDebouncedQuery(query), 100);
    return () => clearTimeout(id);
  }, [query]);

  const parsed = useMemo<ParsedLogLine[]>(() => lines.map(parseCollectorLog), [lines]);
  const filtered = useMemo<ParsedLogLine[]>(() => {
    const q = debouncedQuery.trim().toLowerCase();
    if (!q) return parsed;
    return parsed.filter((line) => line.raw.toLowerCase().includes(q));
  }, [parsed, debouncedQuery]);

  const visible = showAll ? filtered : filtered.slice(Math.max(0, filtered.length - VISIBLE_CAP));

  // Auto-tail: pin scroll to bottom as new lines stream in. The user
  // unsticks by scrolling up; the floating arrow re-pins on click.
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const isProgrammaticScroll = useRef(false);
  const [stickToBottom, setStickToBottom] = useState(true);

  useLayoutEffect(() => {
    if (!stickToBottom) return;
    const el = scrollRef.current;
    if (!el) return;
    isProgrammaticScroll.current = true;
    el.scrollTop = el.scrollHeight;
  }, [visible.length, stickToBottom]);

  const handleScroll = (): void => {
    if (isProgrammaticScroll.current) {
      isProgrammaticScroll.current = false;
      return;
    }
    const el = scrollRef.current;
    if (!el) return;
    const distanceFromBottom = el.scrollHeight - el.clientHeight - el.scrollTop;
    const nearBottom = distanceFromBottom < 24;
    setStickToBottom((prev) => (prev === nearBottom ? prev : nearBottom));
  };

  const jumpToBottom = (): void => {
    const el = scrollRef.current;
    if (el) {
      isProgrammaticScroll.current = true;
      el.scrollTop = el.scrollHeight;
    }
    setStickToBottom(true);
  };

  const hasQuery = debouncedQuery.trim().length > 0;
  const counter = loading
    ? 'Loading…'
    : hasQuery
      ? `${filtered.length} of ${parsed.length} lines`
      : `${parsed.length} lines`;

  return (
    <section
      data-testid="logs-panel"
      className="flex flex-col flex-1 min-h-0 rounded-md border border-slate-200 bg-white px-3 py-2 dark:border-slate-800 dark:bg-slate-900"
    >
      <header className="mb-2 flex flex-shrink-0 items-center gap-2">
        <h2 className="text-sm font-semibold text-slate-900 dark:text-slate-100">Logs</h2>
        <div className="relative ml-2 flex-1">
          <Search
            size={12}
            aria-hidden="true"
            className="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-slate-400"
          />
          <input
            type="text"
            role="searchbox"
            data-testid="logs-search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Filter…"
            aria-label="Filter logs"
            className="w-full rounded-md border border-slate-300 bg-white py-1 pl-7 pr-7 text-xs text-slate-800 shadow-sm focus:border-blue-500 focus:outline-none focus:ring-1 focus:ring-blue-500 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-200"
          />
          {hasQuery ? (
            <button
              type="button"
              data-testid="logs-search-clear"
              aria-label="Clear filter"
              onClick={() => setQuery('')}
              className="absolute right-1.5 top-1/2 -translate-y-1/2 rounded p-0.5 text-slate-400 hover:text-slate-700 dark:hover:text-slate-200"
            >
              <X size={12} aria-hidden="true" />
            </button>
          ) : null}
        </div>
        <span
          data-testid="logs-counter"
          className="flex-shrink-0 text-xs tabular-nums text-slate-500 dark:text-slate-400"
        >
          {counter}
        </span>
      </header>

      <div
        ref={scrollRef}
        onScroll={handleScroll}
        role="log"
        aria-label="Collector log"
        data-testid="logs-output"
        className="relative flex-1 min-h-0 overflow-auto rounded bg-slate-50 px-2 py-1 font-mono text-[11px] leading-snug dark:bg-slate-950"
      >
        {visible.length === 0 ? (
          <p className="text-slate-500 dark:text-slate-400">
            {loading
              ? 'Loading…'
              : hasQuery
                ? `No lines match “${debouncedQuery.trim()}”.`
                : '(no log output yet)'}
          </p>
        ) : (
          <>
            {!showAll && filtered.length > VISIBLE_CAP ? (
              <div className="mb-1 flex items-center justify-between rounded border border-slate-200 bg-white px-2 py-1 text-[10px] text-slate-500 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-400">
                <span>
                  Showing the most recent {VISIBLE_CAP} of {filtered.length} lines.
                </span>
                <button
                  type="button"
                  data-testid="logs-show-all"
                  onClick={() => setShowAll(true)}
                  className="rounded px-1.5 py-0.5 font-medium text-blue-700 hover:bg-blue-50 dark:text-blue-300 dark:hover:bg-slate-800"
                >
                  Show all
                </button>
              </div>
            ) : null}
            {visible.map((line, idx) => (
              <LogRow key={idx} line={line} />
            ))}
          </>
        )}
        {!stickToBottom && filtered.length > 0 ? (
          <button
            type="button"
            data-testid="logs-jump-to-bottom"
            onClick={jumpToBottom}
            aria-label="Jump to latest"
            title="Jump to latest"
            className="sticky bottom-2 ml-auto flex h-7 w-7 items-center justify-center rounded-full border border-slate-300 bg-white text-slate-700 shadow-md hover:bg-slate-100 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200 dark:hover:bg-slate-700"
            style={{ float: 'right' }}
          >
            <ArrowDown size={14} aria-hidden="true" />
          </button>
        ) : null}
      </div>
    </section>
  );
}

function LogRow({ line }: { line: ParsedLogLine }): JSX.Element {
  const time = formatLogTime(line.timestamp);
  const levelClass = LEVEL_STYLE[line.level];
  return (
    <div
      data-stream={line.stream}
      data-level={line.level}
      className="flex gap-2 whitespace-pre-wrap break-words py-px"
    >
      {time ? (
        <span className="flex-shrink-0 text-slate-500 dark:text-slate-500">{time}</span>
      ) : (
        <span className="flex-shrink-0 text-slate-400 dark:text-slate-600"> </span>
      )}
      <span className={`flex-shrink-0 font-semibold ${levelClass}`}>
        {LEVEL_LABEL[line.level].padEnd(5)}
      </span>
      <span className={`min-w-0 ${levelClass}`}>{line.message}</span>
    </div>
  );
}

import { ArrowDown, Search, X } from 'lucide-react';
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';

import { useCollectorLogTail } from '../hooks/useCollectorLogTail.js';
import {
  type LogLevel,
  type ParsedLogLine,
  formatLogTime,
  parseCollectorLog,
} from '../lib/parseCollectorLog.js';
import { Button, Card, CardTitle } from './ui/index.js';

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
  debug: 'text-fg-tertiary dark:text-fg-tertiary-dark',
  info: 'text-fg-primary dark:text-fg-primary-dark',
  warn: 'text-ios-orange',
  error: 'text-ios-red',
};

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
    <Card testid="logs-panel" padding="sm" className="flex flex-1 min-h-0 flex-col">
      <header className="mb-2 flex flex-shrink-0 items-center gap-2">
        <CardTitle>Logs</CardTitle>
        <div className="relative ml-2 flex-1">
          <Search
            size={12}
            aria-hidden="true"
            className="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-fg-tertiary dark:text-fg-tertiary-dark"
          />
          <input
            type="text"
            role="searchbox"
            data-testid="logs-search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Filter…"
            aria-label="Filter logs"
            className="w-full rounded-[8px] border border-hairline bg-surface-elevated py-1 pl-7 pr-7 text-[12px] text-fg-primary placeholder:text-fg-tertiary focus:border-ios-blue focus:outline-none focus:ring-1 focus:ring-ios-blue dark:border-hairline-dark dark:bg-surface-elevated-dark dark:text-fg-primary-dark dark:placeholder:text-fg-tertiary-dark"
          />
          {hasQuery ? (
            <button
              type="button"
              data-testid="logs-search-clear"
              aria-label="Clear filter"
              onClick={() => setQuery('')}
              className="absolute right-1.5 top-1/2 -translate-y-1/2 rounded p-0.5 text-fg-tertiary hover:text-fg-primary dark:text-fg-tertiary-dark dark:hover:text-fg-primary-dark"
            >
              <X size={12} aria-hidden="true" />
            </button>
          ) : null}
        </div>
        <span
          data-testid="logs-counter"
          className="flex-shrink-0 text-[11px] tabular-nums text-fg-tertiary dark:text-fg-tertiary-dark"
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
        className="relative min-h-0 flex-1 overflow-auto rounded-[8px] bg-canvas px-2 py-1 font-mono text-[11px] leading-snug dark:bg-canvas-dark"
      >
        {visible.length === 0 ? (
          <p className="text-fg-tertiary dark:text-fg-tertiary-dark">
            {loading
              ? 'Loading…'
              : hasQuery
                ? `No lines match “${debouncedQuery.trim()}”.`
                : '(no log output yet)'}
          </p>
        ) : (
          <>
            {!showAll && filtered.length > VISIBLE_CAP ? (
              <div className="mb-1 flex items-center justify-between rounded-[6px] border border-hairline bg-surface-elevated px-2 py-1 text-[10px] text-fg-tertiary dark:border-hairline-dark dark:bg-surface-elevated-dark dark:text-fg-tertiary-dark">
                <span>
                  Showing the most recent {VISIBLE_CAP} of {filtered.length} lines.
                </span>
                <Button
                  variant="ghost"
                  size="sm"
                  testid="logs-show-all"
                  onClick={() => setShowAll(true)}
                >
                  Show all
                </Button>
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
            className="sticky bottom-2 ml-auto flex h-7 w-7 items-center justify-center rounded-full border border-hairline bg-surface-elevated text-fg-primary shadow-card hover:bg-canvas dark:border-hairline-dark dark:bg-surface-elevated-dark dark:text-fg-primary-dark dark:hover:bg-canvas-dark"
            style={{ float: 'right' }}
          >
            <ArrowDown size={14} aria-hidden="true" />
          </button>
        ) : null}
      </div>
    </Card>
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
        <span className="flex-shrink-0 text-fg-tertiary dark:text-fg-tertiary-dark">{time}</span>
      ) : (
        <span className="flex-shrink-0 text-fg-quaternary dark:text-fg-quaternary-dark"> </span>
      )}
      <span className={`flex-shrink-0 font-semibold ${levelClass}`}>
        {LEVEL_LABEL[line.level].padEnd(5)}
      </span>
      <span className={`min-w-0 ${levelClass}`}>{line.message}</span>
    </div>
  );
}

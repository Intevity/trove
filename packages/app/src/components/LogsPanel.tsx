import { ArrowDown, Check, Copy, Search, X } from 'lucide-react';
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';

import { useCollectorLogTail } from '../hooks/useCollectorLogTail.js';
import { copyToClipboard } from '../lib/clipboard.js';
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
  debug: 'DBG',
  info: 'INF',
  warn: 'WRN',
  error: 'ERR',
};

/** Translucent pill behind the 3-letter level token. Brand teal for
 *  the steady-state INFO line, iOS amber/red for problems, neutral
 *  for DEBUG noise. */
const LEVEL_BADGE: Record<LogLevel, string> = {
  debug: 'bg-black/[0.06] text-fg-tertiary dark:bg-white/[0.08] dark:text-fg-tertiary-dark',
  info: 'bg-brand/[0.16] text-brand',
  warn: 'bg-ios-orange/[0.16] text-ios-orange',
  error: 'bg-ios-red/[0.18] text-ios-red',
};

/** Left-border rail — colours the row's edge without repainting the
 *  whole line. Debug/info stay transparent so the panel reads quiet;
 *  warn/error pull the eye. */
const ROW_RAIL: Record<LogLevel, string> = {
  debug: 'border-l-transparent',
  info: 'border-l-transparent',
  warn: 'border-l-ios-orange/50',
  error: 'border-l-ios-red/60',
};

/** Whisper-thin row tint for severe rows — stays under 9% alpha so
 *  multi-line tracebacks don't become a wall of colour. */
const ROW_TINT: Record<LogLevel, string> = {
  debug: '',
  info: '',
  warn: 'bg-ios-orange/[0.04] dark:bg-ios-orange/[0.07]',
  error: 'bg-ios-red/[0.05] dark:bg-ios-red/[0.09]',
};

/** 12 spaces — matches "HH:MM:SS.mmm" render width so the level badge
 *  column stays aligned across rows with and without a timestamp. */
const EMPTY_TIME_PLACEHOLDER = '            ';

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
  const counterClass = hasQuery
    ? 'flex-shrink-0 text-[11px] font-medium tabular-nums text-brand'
    : 'flex-shrink-0 text-[11px] tabular-nums text-fg-tertiary dark:text-fg-tertiary-dark';

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
            className="w-full rounded-[8px] border border-hairline bg-surface-elevated py-1 pl-7 pr-7 text-[12px] text-fg-primary placeholder:text-fg-tertiary focus:border-brand focus:outline-none focus:ring-1 focus:ring-brand dark:border-hairline-dark dark:bg-surface-elevated-dark dark:text-fg-primary-dark dark:placeholder:text-fg-tertiary-dark"
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
        <span data-testid="logs-counter" className={counterClass}>
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
  const [copied, setCopied] = useState(false);
  const handleCopy = (): void => {
    void copyToClipboard(line.raw).then((ok) => {
      if (!ok) return;
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1200);
    });
  };
  return (
    <div
      data-stream={line.stream}
      data-level={line.level}
      className={`group flex items-baseline gap-2 whitespace-pre-wrap break-words border-l-2 py-[1px] pl-2 pr-1 transition-colors hover:bg-black/[0.03] dark:hover:bg-white/[0.04] ${ROW_RAIL[line.level]} ${ROW_TINT[line.level]}`}
    >
      <span className="flex-shrink-0 tabular-nums text-fg-tertiary dark:text-fg-tertiary-dark">
        {time || EMPTY_TIME_PLACEHOLDER}
      </span>
      <span
        className={`inline-flex flex-shrink-0 items-center justify-center rounded-[4px] px-1.5 py-[1px] text-[10px] font-semibold uppercase tracking-wider ${LEVEL_BADGE[line.level]}`}
      >
        {LEVEL_LABEL[line.level]}
      </span>
      <span className="min-w-0 flex-1 text-fg-primary dark:text-fg-primary-dark">
        {line.message}
      </span>
      <button
        type="button"
        onClick={handleCopy}
        aria-label={copied ? 'Log line copied' : 'Copy log line'}
        title={copied ? 'Copied' : 'Copy log line'}
        data-testid="logs-copy-line"
        className={`flex-shrink-0 self-center rounded p-0.5 transition-opacity hover:bg-black/[0.06] dark:hover:bg-white/[0.08] ${
          copied
            ? 'text-ios-green opacity-100'
            : 'text-fg-tertiary opacity-0 hover:text-fg-primary group-hover:opacity-100 dark:text-fg-tertiary-dark dark:hover:text-fg-primary-dark'
        }`}
      >
        {copied ? <Check size={11} aria-hidden="true" /> : <Copy size={11} aria-hidden="true" />}
      </button>
    </div>
  );
}

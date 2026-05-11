import { open } from '@tauri-apps/plugin-shell';

import type { CollectorLogLineWire } from '@trove/shared';

const REPO = 'Intevity/trove';
const ISSUE_URL = `https://github.com/${REPO}/issues/new`;

// GitHub URLs hold up to ~8KB before browsers / servers start complaining.
// Cap the body under that so the URL, title, and encoding overhead always fit.
const MAX_BODY_CHARS = 6000;

export type ReportSource = 'manual' | 'error-boundary' | 'collector-error';

export interface ReportError {
  message: string;
  stack?: string;
  componentStack?: string;
}

export interface BugReportContext {
  source: ReportSource;
  error?: ReportError;
  collectorLogLines?: CollectorLogLineWire[];
}

interface EnvInfo {
  appVersion: string;
  userAgent: string;
}

function collectEnv(): EnvInfo {
  return {
    appVersion: __APP_VERSION__,
    userAgent: typeof navigator !== 'undefined' ? navigator.userAgent : 'unknown',
  };
}

function formatLogLine(e: CollectorLogLineWire): string {
  return `[${e.stream}] ${e.line}`;
}

export function buildTitle(ctx: BugReportContext): string {
  if (ctx.source === 'error-boundary' && ctx.error) {
    const firstLine = ctx.error.message.split('\n')[0]?.slice(0, 120) ?? '';
    return `[crash] ${firstLine}`;
  }
  if (
    ctx.source === 'collector-error' &&
    ctx.collectorLogLines &&
    ctx.collectorLogLines.length > 0
  ) {
    const latest = ctx.collectorLogLines[ctx.collectorLogLines.length - 1]!;
    const firstLine = latest.line.split('\n')[0]?.slice(0, 120) ?? '';
    return `[collector] ${firstLine}`.trim();
  }
  return '[bug] ';
}

interface BodySection {
  // Fixed sections always render. Truncatable sections can be shortened
  // (logs first, stack second) to keep the body under MAX_BODY_CHARS.
  kind: 'fixed' | 'logs' | 'stack';
  content: string;
}

function envSection(env: EnvInfo): string {
  return ['## Environment', `- Trove: ${env.appVersion}`, `- User agent: ${env.userAgent}`].join(
    '\n',
  );
}

function logsSection(entries: CollectorLogLineWire[]): string {
  const lines = entries.slice(-20).map(formatLogLine).join('\n');
  return [
    '<details><summary>Recent collector log (last 20 lines)</summary>',
    '',
    '```',
    lines,
    '```',
    '',
    '</details>',
  ].join('\n');
}

function stackSection(error: ReportError): string {
  const parts = [error.message];
  if (error.stack) {
    parts.push('', error.stack);
  }
  if (error.componentStack) {
    parts.push('', 'Component stack:', error.componentStack);
  }
  return [
    '<details><summary>UI error & stack</summary>',
    '',
    '```',
    parts.join('\n'),
    '```',
    '',
    '</details>',
  ].join('\n');
}

function truncateTail(s: string, maxChars: number): string {
  if (s.length <= maxChars) return s;
  const keep = Math.max(0, maxChars - 32);
  return `…[truncated ${s.length - keep} chars]…\n${s.slice(-keep)}`;
}

export function buildBody(ctx: BugReportContext): string {
  const env = collectEnv();
  const intro =
    ctx.source === 'error-boundary'
      ? '<!-- The app hit a render error. Add any context that helps us reproduce. -->'
      : ctx.source === 'collector-error'
        ? '<!-- Recent collector errors were detected. Add any context that helps us reproduce. -->'
        : '<!-- Describe the problem here. -->';

  const sections: BodySection[] = [
    { kind: 'fixed', content: intro },
    { kind: 'fixed', content: '## Steps to reproduce\n1. \n2. \n3. ' },
    { kind: 'fixed', content: '## Expected behavior\n' },
    { kind: 'fixed', content: '## Actual behavior\n' },
    { kind: 'fixed', content: envSection(env) },
  ];

  if (ctx.collectorLogLines && ctx.collectorLogLines.length > 0) {
    sections.push({ kind: 'logs', content: logsSection(ctx.collectorLogLines) });
  }
  if (ctx.error) {
    sections.push({ kind: 'stack', content: stackSection(ctx.error) });
  }

  const join = (parts: BodySection[]): string => parts.map((s) => s.content).join('\n\n');
  let body = join(sections);
  if (body.length <= MAX_BODY_CHARS) return body;

  // Over budget — shrink logs first.
  const logsIdx = sections.findIndex((s) => s.kind === 'logs');
  if (logsIdx >= 0) {
    const others = sections
      .filter((_, i) => i !== logsIdx)
      .reduce((n, s) => n + s.content.length + 2, 0);
    const budget = Math.max(200, MAX_BODY_CHARS - others);
    sections[logsIdx] = {
      kind: 'logs',
      content: truncateTail(sections[logsIdx]!.content, budget),
    };
    body = join(sections);
    if (body.length <= MAX_BODY_CHARS) return body;
  }

  const stackIdx = sections.findIndex((s) => s.kind === 'stack');
  if (stackIdx >= 0) {
    const others = sections
      .filter((_, i) => i !== stackIdx)
      .reduce((n, s) => n + s.content.length + 2, 0);
    const budget = Math.max(200, MAX_BODY_CHARS - others);
    sections[stackIdx] = {
      kind: 'stack',
      content: truncateTail(sections[stackIdx]!.content, budget),
    };
    body = join(sections);
  }

  return body.length <= MAX_BODY_CHARS ? body : truncateTail(body, MAX_BODY_CHARS);
}

export function buildIssueUrl(ctx: BugReportContext): string {
  const params = new URLSearchParams({
    title: buildTitle(ctx),
    body: buildBody(ctx),
    labels: 'bug',
  });
  return `${ISSUE_URL}?${params.toString()}`;
}

export async function openBugReport(ctx: BugReportContext): Promise<void> {
  await open(buildIssueUrl(ctx));
}

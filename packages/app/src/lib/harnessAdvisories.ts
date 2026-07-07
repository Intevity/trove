import type { HarnessId } from '@trove/shared';

/** Short user-facing instruction surfaced after a successful Enable for
 *  harnesses whose runtime can't pick up Trove's patch on its own. Empty
 *  entries (missing keys) mean no toast — Claude Code, Codex CLI, Qwen
 *  Code, OpenCode, and Cline all read their config or watch their files
 *  freshly on each invocation, so no user action is needed.
 *
 *  Cursor (IDE + CLI) and Antigravity CLI keep their hooks.json snapshot
 *  in memory across the whole session; a restart is the only way they
 *  pick up newly installed hook entries. Aider and Copilot CLI install a
 *  shell-rc function that only takes effect in *new* shells. */
export const POST_ENABLE_ADVISORIES: Partial<Record<HarnessId, string>> = {
  'cursor-ide':
    'Quit and reopen Cursor — the IDE only loads hooks.json at startup, so the Trove hook won’t fire in this session.',
  'cursor-cli': 'Restart any open Cursor CLI sessions — the CLI only loads hooks.json at startup.',
  'antigravity-cli':
    'Restart any open `agy` sessions — Antigravity only loads hooks.json at startup, and hooks fire in interactive mode (not `agy -p`).',
  aider:
    'Open a new terminal — Trove appended a shell function to your rc file, and existing shells won’t see it until they source it.',
  'copilot-cli':
    'Open a new terminal and invoke `gh-copilot` (with a hyphen) instead of `gh copilot`. Existing shells won’t see the new function until they source the rc file.',
};

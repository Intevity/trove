# `resources/hooks/`

Vendored hook / plugin code that Trove ships alongside the app and
references at runtime. Files in this tree get bundled into the
distributable via the `bundle.resources` entry in
`packages/app/src-tauri/tauri.conf.json` and resolved at runtime via
`tauri::path::PathResolver::resource_dir()`.

## Layout convention

Each hook or plugin lives in its own file or directory at the top
level of this folder. Use kebab-case names matching the harness:

```
resources/hooks/
├── cursor-otel-hook.cjs       # single-file Node hook (Cursor IDE / CLI)
├── README.md                  # this file
└── <future>/                  # future Tier 3 wrappers (Sprint 9)
```

A bundled hook should:

- Be **stdlib-only** if practical. The Cursor hook is single-file
  Node using only `node:http` so we don't carry a dependency tree.
- **Exit 0 on any error**. Hooks are invoked as subprocesses by the
  host harness; a non-zero exit blocks the user's workflow.
- **Be idempotent / repeatable**. The host harness may invoke the
  same hook many times during one session.
- **Provide a `--health` smoke flag** that prints `ok` and exits 0
  without doing real work. Tests use this to verify the binary is
  executable and on the expected path.

## What lives here vs what doesn't

- **Bundled** — the file ships with Trove, gets staged into the
  user's machine via Tauri's resource pipeline, and is referenced
  by absolute path in the harness's host config. Cursor's
  `cursor-otel-hook.cjs` is the canonical example.
- **Not bundled** — npm-resolved plugins (OpenCode's
  `@devtheops/opencode-plugin-otel`) live in the upstream registry,
  not in this tree. Trove only writes the package name into
  `opencode.json`; OpenCode's own runtime resolves and installs it.
  See `documentation/adding-a-harness.md`'s "Plugin / hook style
  adapters (Tier 2)" section for the trade-offs.

## Adding a new bundled file

1. Drop the file at `resources/hooks/<name>.<ext>` (or a directory
   for multi-file plugins).
2. Add the file to `bundle.resources` in
   `packages/app/src-tauri/tauri.conf.json` with the path it should
   land under at runtime.
3. In the adapter, resolve the runtime path with
   `app.path().resource_dir()?.join("resources").join("hooks").join(<name>)`.
4. Document the file's purpose, runtime requirements, and exit
   contract in a comment at the top of the file itself.

## Updating an existing hook

Hooks shipped here are user-facing — installed paths point at the
exact bytes we ship. Any change to a hook's apparent absolute path
or canonical contents invalidates already-installed managed regions
in users' host configs and will surface as a `RegionConflict` on
their next preview. Plan upgrades accordingly: usually rev the
adapter's `build_region` so that re-applying picks up the new shape,
and rely on Sprint 8's three-way merge UI to walk users through the
conflict.

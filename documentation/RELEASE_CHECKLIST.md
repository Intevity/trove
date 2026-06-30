# Release Checklist

Human QA gate to walk before tagging a release. The tag/signing runbook
itself lives in [`releasing.md`](releasing.md); the architectural context
for what these checks exercise lives in [`architecture.md`](architecture.md).

Convention: copy this file into the release tracking issue and tick the
boxes as you go.

---

## General

- [ ] Code signing and automatic-update testing — follow the [Sprint 10 verification playbook](releasing.md#sprint-10-verification-playbook) in `releasing.md`.
- [ ] Windows testing — installer runs, app launches, tray icon appears.
- [ ] Linux testing — both the `.AppImage` and the `.deb` install and launch cleanly.
- [ ] Thoroughly test mappings — editable mappings and custom metrics still emit correctly after apply.

## App UX flows

- [ ] First-run wizard end-to-end: preset picker → credentials form → test export → save.
- [ ] Dashboard health indicator turns green when data flows to the configured backend.
- [ ] Conflict resolver (3-pane `apply_patch` view) renders and resolves cleanly when a region conflict is forced.
- [ ] Settings persist across quit and relaunch (auto-update toggle, theme, configured backend).
- [ ] Tray icon and context menu work on macOS, Windows, and Linux.
- [ ] Light and dark mode both render correctly across every view.

## Apply / revert correctness

- [ ] Apply a harness → patched config parses and validates.
- [ ] Revert → original config restored byte-for-byte.
- [ ] Apply when already applied → idempotent no-op.
- [ ] Revert when not applied → clean no-op.
- [ ] Custom mappings and custom metrics still emit after a full apply / revert cycle.

## Sidecar, credentials, network

- [ ] Collector sidecar spawns, is supervised, respawns on crash, restarts on YAML reload, and shuts down gracefully on app quit.
- [ ] Keychain storage works on macOS Keychain, Windows Credential Manager, and Linux Secret Service.
- [ ] An unreachable backend surfaces a clear error rather than crashing or hanging the UI.
- [ ] Offline launch — app opens and the settings UI is usable with no network.
- [ ] Verify no phoning home — telemetry leaves the machine only toward the user-configured endpoint.

## Harness testing

For each harness, perform: apply → dashboard turns green → revert → dashboard returns to neutral. (See the Tier 1/2 smoke procedure in [`releasing.md`](releasing.md#pre-release-checklist).)

- [ ] Claude Code
- [ ] Claude Desktop
- [ ] Antigravity CLI
- [ ] Cursor IDE
- [ ] Cursor CLI
- [ ] Codex
- [ ] Qwen
- [ ] OpenCode
- [ ] Cline
- [ ] Aider
- [ ] Copilot CLI
- [ ] Junie CLI
- [ ] Droid (factory.ai)
- [ ] Kimi Code CLI
- [ ] Devin
- [ ] ForgeCode

## Platform testing

For each backend, configure credentials, send a test export from Trove, and confirm receipt on the backend side.

- [ ] Per-platform Disable / Enable round-trip — with at least one platform configured: click **Disable**, confirm the row dims and "· disabled" appears, the collector stops forwarding to it (verify on the backend side), and the platform vanishes from the Overview Data flow chart. Click **Enable** and confirm forwarding resumes and the chart picks it up again. Apply / revert credentials on a disabled platform should preserve its disabled state across the wizard Save.
- [ ] SigNoz
- [ ] Grafana
- [ ] Honeycomb
- [ ] Datadog
- [ ] NewRelic
- [ ] Splunk
- [ ] Dynatrace
- [ ] Elastic
- [ ] OpenSearch
- [ ] OpenObserve
- [ ] ClickStack
- [ ] Generic OTLP
- [ ] Local Collector (passthrough)

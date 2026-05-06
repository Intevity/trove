# Security

> _Skeleton document — full threat model is finalized in Sprint 11 of the [MVP plan](documentation/MVP_PLAN.md)._

## Privacy promise

Trove never sends telemetry to a Trove-controlled endpoint. All emitted signals are forwarded only to the observability backend the user configures. The application is designed to work entirely offline once installed.

## Scope (preliminary)

Trove reads and writes:

- Per-harness configuration files in the user's home directory (e.g. `~/.claude/settings.json`, `~/.gemini/settings.json`, `~/.codex/config.toml`).
- Its own state file under the OS-appropriate config directory.
- Backend credentials in the OS keychain (never in plaintext on disk).
- The bundled Collector's YAML config and log file.

Trove does **not** read source code, prompts, or user files outside the directories listed above.

## Reporting a vulnerability

Please do not open a public GitHub issue for security reports. Email the maintainer at `jeff.wooden@intevity.com` with a description of the issue and reproduction steps. Response within 5 business days.

## Threat model (TBD)

A complete threat model — what the app can and cannot see, what it touches, what it sends where, how to revoke — will land in Sprint 11 alongside the 1.0 release.

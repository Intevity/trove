//! The set of AI coding harnesses Trove targets.
//!
//! Mirrors the Zod `HarnessId` enum in `packages/shared/src/schemas.ts`.
//! Variants serialize as kebab-case strings so the wire format matches
//! the TS Zod definition byte-for-byte.

use serde::{Deserialize, Serialize};

/// Discriminated union of supported AI coding harness identifiers.
/// Sprint 3 only detects + patches Tier 1 (the first four variants).
/// Tier 2 detection lands in Sprint 7; Tier 3 in Sprint 9.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HarnessId {
    ClaudeCode,
    ClaudeDesktop,
    /// Google Antigravity CLI (`agy`), the successor to the discontinued
    /// Gemini CLI. Antigravity dropped the native OTLP exporter Gemini CLI
    /// had, so Trove integrates it as a Tier 2 hook harness (a Trove-shipped
    /// hook bridge, like Cursor) rather than a native-OTEL settings patch.
    /// The wire id is `antigravity-cli`.
    AntigravityCli,
    CodexCli,
    /// `OpenAI` Codex desktop app (`/Applications/Codex.app`). Shares
    /// `~/.codex/config.toml` with the CLI — both invoke the same Rust
    /// `codex app-server` backend — so the codex-desktop adapter writes
    /// the same `[otel.*]` payload as codex-cli. The two adapters
    /// coexist in the shared TOML block via dep-tracking (see
    /// `safety::sentinels::comment_fence`).
    CodexDesktop,
    QwenCode,
    Opencode,
    CursorIde,
    CursorCli,
    Cline,
    Aider,
    CopilotCli,
    /// `JetBrains` Junie CLI. Detection-only — no native `OTEL` today, so
    /// `has_adapter()` returns false and the UI shows the row disabled.
    JunieCli,
    /// `factory.ai` Droid CLI. Detection-only.
    Droid,
    /// Moonshot AI Kimi Code CLI. Detection-only.
    KimiCodeCli,
    /// Cognition Devin CLI. Detection-only.
    Devin,
    /// `ForgeCode` CLI. Detection-only.
    Forgecode,
    /// Claude Sentinel — a Claude Code companion that collects and
    /// enriches Claude Code telemetry, then forwards it onward. It is a
    /// telemetry *source* for Trove (Claude Code → Sentinel → Trove), but
    /// detection-only here: Sentinel owns its own integrity-signed
    /// forwarder config and points itself at Trove's collector, so there
    /// is no host file for Trove to patch. The row is informational; the
    /// wiring lives in Sentinel's own settings (its "Forward to Trove").
    Sentinel,
}

impl HarnessId {
    /// Human-readable label for UI surfaces. Matches the names used in
    /// the MVP plan's harness table.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::ClaudeDesktop => "Claude Desktop",
            Self::AntigravityCli => "Antigravity CLI",
            Self::CodexCli => "OpenAI Codex CLI",
            Self::CodexDesktop => "OpenAI Codex",
            Self::QwenCode => "Qwen Code",
            Self::Opencode => "OpenCode",
            Self::CursorIde => "Cursor IDE",
            Self::CursorCli => "Cursor CLI",
            Self::Cline => "Cline",
            Self::Aider => "Aider",
            Self::CopilotCli => "GitHub Copilot CLI",
            Self::JunieCli => "Junie CLI",
            Self::Droid => "Droid",
            Self::KimiCodeCli => "Kimi Code CLI",
            Self::Devin => "Devin",
            Self::Forgecode => "ForgeCode",
            Self::Sentinel => "Sentinel",
        }
    }

    /// Tier 1 harnesses — those with native OTEL the configurator knows
    /// how to detect today. Sprint 3 ships detection + adapters for the
    /// Every supported harness id, in declaration order. Convenient for
    /// iteration where the tier split isn't relevant (e.g. mapping a
    /// codegen-name suffix back to a [`HarnessId`]).
    #[must_use]
    pub fn all() -> &'static [Self] {
        &[
            Self::ClaudeCode,
            Self::ClaudeDesktop,
            Self::AntigravityCli,
            Self::CodexCli,
            Self::CodexDesktop,
            Self::QwenCode,
            Self::Opencode,
            Self::CursorIde,
            Self::CursorCli,
            Self::Cline,
            Self::Aider,
            Self::CopilotCli,
            // Detection-only harnesses: appear in the dashboard with the
            // toggle disabled (has_adapter() returns false because they
            // are in none of the tier_{1,2,3} arrays).
            Self::JunieCli,
            Self::Droid,
            Self::KimiCodeCli,
            Self::Devin,
            Self::Forgecode,
            Self::Sentinel,
        ]
    }

    /// first batch; Claude Desktop (formerly Claude Cowork) is detected
    /// here too but has no local adapter because its OTLP setup lives in
    /// the Claude admin web UI rather than a config file Trove can patch.
    /// See [`Self::has_adapter`].
    #[must_use]
    pub fn tier_1() -> &'static [Self] {
        &[
            Self::ClaudeCode,
            Self::ClaudeDesktop,
            Self::Droid,
            Self::CodexCli,
            Self::CodexDesktop,
            Self::QwenCode,
        ]
    }

    /// Returns the metric-name prefix used for OTLP filtering and resource
    /// tagging in the collector when the harness's SDK ignores
    /// `OTEL_RESOURCE_ATTRIBUTES` and `service.name` is too generic to
    /// filter on. Returns `None` for harnesses where standard
    /// `service.name` matching works.
    ///
    /// When `Some(prefix)` is returned, the collector's
    /// `transform/harness-tag` processor uses `IsMatch(name, "^{prefix}\.")`
    /// in a `metrics` OTTL context, and `filter/diag-*` uses
    /// `not IsMatch(name, "^{prefix}\.")` in the `metrics.metric` context.
    #[must_use]
    pub fn metric_name_tag_prefix(self) -> Option<&'static str> {
        match self {
            // factory.ai's SDK hardcodes `service.name=cli` and ignores
            // `OTEL_RESOURCE_ATTRIBUTES`, so we match on the `droid.*`
            // metric-name namespace instead.
            Self::Droid => Some("droid"),
            _ => None,
        }
    }

    /// Tier 2 harnesses — those that need a Trove-shipped hook or
    /// plugin instead of a native OTEL toggle. Sprint 7 PR 1 landed the
    /// two Cursor variants; PR 2 appended `OpenCode`. Antigravity CLI
    /// joined here when Google dropped Gemini CLI's native OTLP exporter:
    /// it now rides a Trove-shipped hook bridge (see `antigravity_cli`),
    /// exactly like the Cursor harnesses.
    #[must_use]
    pub fn tier_2() -> &'static [Self] {
        &[
            Self::CursorIde,
            Self::CursorCli,
            Self::Opencode,
            Self::AntigravityCli,
        ]
    }

    /// Tier 3 harnesses — best-effort adapters that have no native OTEL
    /// surface. Sprint 9 lands them: PR 1 (this) makes the trio visible
    /// in detection so the UI shows 10 rows; PR 2 wires Cline; PR 3
    /// wires Aider + Copilot CLI. Each adapter flips its own
    /// `has_adapter()` bit when it lands.
    #[must_use]
    pub fn tier_3() -> &'static [Self] {
        &[Self::Cline, Self::Aider, Self::CopilotCli]
    }

    /// Whether Trove currently ships an adapter for this harness. The
    /// dashboard surfaces this as part of each `DetectedHarness` row —
    /// `false` disables the Enable/Disable toggle so the user knows the
    /// row is informational only.
    ///
    /// Every Tier 1 / 2 / 3 harness has an adapter today. Claude Desktop
    /// is adapter-backed by an audit-log tap (no host config to patch),
    /// modelled on Cline's `preview`/`apply`/`revert` shape; toggling
    /// the row simply spawns or aborts the tap.
    #[must_use]
    pub fn has_adapter(self) -> bool {
        Self::tier_1().contains(&self)
            || Self::tier_2().contains(&self)
            || Self::tier_3().contains(&self)
    }

    /// Whether enabling this harness only spawns a watcher rather than
    /// writing a managed region into a host config file. For these,
    /// `trove_region_present` on a freshly-detected row is always
    /// `false` (detection scans the host file, which doesn't carry a
    /// Trove region for watcher-only harnesses), so the dashboard
    /// derives the enabled state from `state.json` instead.
    ///
    /// Today this covers all of Tier 3 (Cline, Aider, Copilot CLI) plus
    /// Claude Desktop, whose audit-log-tap adapter has no host file to
    /// patch.
    #[must_use]
    pub fn enables_via_watcher_only(self) -> bool {
        Self::tier_3().contains(&self) || matches!(self, Self::ClaudeDesktop)
    }
}

#[cfg(test)]
mod tests {
    use super::HarnessId;

    #[test]
    fn serializes_as_kebab_case() {
        let json = serde_json::to_string(&HarnessId::ClaudeCode).unwrap();
        assert_eq!(json, "\"claude-code\"");
        let json = serde_json::to_string(&HarnessId::CopilotCli).unwrap();
        assert_eq!(json, "\"copilot-cli\"");
    }

    #[test]
    fn deserializes_kebab_case() {
        let id: HarnessId = serde_json::from_str("\"antigravity-cli\"").unwrap();
        assert_eq!(id, HarnessId::AntigravityCli);
    }

    #[test]
    fn rejects_unknown_id() {
        assert!(serde_json::from_str::<HarnessId>("\"not-a-harness\"").is_err());
    }

    #[test]
    fn tier_1_lists_native_otel_harnesses_in_plan_order() {
        assert_eq!(
            HarnessId::tier_1(),
            &[
                HarnessId::ClaudeCode,
                HarnessId::ClaudeDesktop,
                HarnessId::Droid,
                HarnessId::CodexCli,
                HarnessId::CodexDesktop,
                HarnessId::QwenCode,
            ]
        );
    }

    #[test]
    fn tier_2_contains_cursor_pair_then_opencode_then_antigravity() {
        // Sprint 7 PR 1 landed the cursor pair; PR 2 appended Opencode.
        // Antigravity CLI joined when Gemini CLI's native OTLP was dropped
        // and it became a Trove-shipped hook harness.
        assert_eq!(
            HarnessId::tier_2(),
            &[
                HarnessId::CursorIde,
                HarnessId::CursorCli,
                HarnessId::Opencode,
                HarnessId::AntigravityCli,
            ]
        );
    }

    #[test]
    fn tiers_do_not_overlap() {
        for t1 in HarnessId::tier_1() {
            assert!(
                !HarnessId::tier_2().contains(t1),
                "{t1:?} appears in both tier_1 and tier_2"
            );
            assert!(
                !HarnessId::tier_3().contains(t1),
                "{t1:?} appears in both tier_1 and tier_3"
            );
        }
        for t2 in HarnessId::tier_2() {
            assert!(
                !HarnessId::tier_3().contains(t2),
                "{t2:?} appears in both tier_2 and tier_3"
            );
        }
    }

    #[test]
    fn tier_3_contains_cline_aider_copilot_in_plan_order() {
        assert_eq!(
            HarnessId::tier_3(),
            &[HarnessId::Cline, HarnessId::Aider, HarnessId::CopilotCli]
        );
    }

    #[test]
    fn has_adapter_is_true_for_every_tier_1_and_tier_2_harness() {
        for id in HarnessId::tier_1().iter().chain(HarnessId::tier_2()) {
            assert!(id.has_adapter(), "{id:?} should have an adapter");
        }
    }

    #[test]
    fn claude_desktop_is_tier_1_and_now_adapter_backed_via_audit_log_tap() {
        // Sprint refactor: ClaudeDesktop moved from "no adapter" to
        // adapter-backed by an audit.jsonl tap (the watcher in
        // `adapters::claude_desktop_watcher`). It still has no host
        // config to patch, but the user can Enable/Disable the tap the
        // same way as every other harness.
        assert!(HarnessId::tier_1().contains(&HarnessId::ClaudeDesktop));
        assert!(HarnessId::ClaudeDesktop.has_adapter());
    }

    #[test]
    fn has_adapter_is_true_for_every_tier_3_harness_after_pr_3() {
        for id in HarnessId::tier_3() {
            assert!(id.has_adapter(), "{id:?} should have an adapter at PR 3");
        }
    }

    #[test]
    fn label_is_present_for_every_variant() {
        // Exhaustive match in label() guarantees coverage; this test
        // documents that no variant returns an empty string by accident.
        for id in [
            HarnessId::ClaudeCode,
            HarnessId::ClaudeDesktop,
            HarnessId::AntigravityCli,
            HarnessId::CodexCli,
            HarnessId::CodexDesktop,
            HarnessId::QwenCode,
            HarnessId::Opencode,
            HarnessId::CursorIde,
            HarnessId::CursorCli,
            HarnessId::Cline,
            HarnessId::Aider,
            HarnessId::CopilotCli,
            HarnessId::JunieCli,
            HarnessId::Droid,
            HarnessId::KimiCodeCli,
            HarnessId::Devin,
            HarnessId::Forgecode,
            HarnessId::Sentinel,
        ] {
            assert!(!id.label().is_empty(), "empty label for {id:?}");
        }
    }

    #[test]
    fn detection_only_harnesses_have_no_adapter() {
        for id in [
            HarnessId::JunieCli,
            HarnessId::KimiCodeCli,
            HarnessId::Devin,
            HarnessId::Forgecode,
            HarnessId::Sentinel,
        ] {
            assert!(!id.has_adapter(), "{id:?} should be detection-only");
        }
    }
}

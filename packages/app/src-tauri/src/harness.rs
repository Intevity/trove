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
    GeminiCli,
    CodexCli,
    QwenCode,
    Opencode,
    CursorIde,
    CursorCli,
    Cline,
    Aider,
    CopilotCli,
    /// JetBrains Junie CLI. Detection-only — no native OTEL today, so
    /// `has_adapter()` returns false and the UI shows the row disabled.
    JunieCli,
    /// factory.ai Droid CLI. Detection-only.
    Droid,
    /// Moonshot AI Kimi Code CLI. Detection-only.
    KimiCodeCli,
    /// Cognition Devin CLI. Detection-only.
    Devin,
    /// ForgeCode CLI. Detection-only.
    Forgecode,
}

impl HarnessId {
    /// Human-readable label for UI surfaces. Matches the names used in
    /// the MVP plan's harness table.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::ClaudeDesktop => "Claude Desktop",
            Self::GeminiCli => "Gemini CLI",
            Self::CodexCli => "OpenAI Codex CLI",
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
            Self::GeminiCli,
            Self::CodexCli,
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
            Self::GeminiCli,
            Self::CodexCli,
            Self::QwenCode,
        ]
    }

    /// Tier 2 harnesses — those that need a Trove-shipped hook or
    /// plugin instead of a native OTEL toggle. Sprint 7 PR 1 landed the
    /// two Cursor variants; PR 2 appended `OpenCode`.
    #[must_use]
    pub fn tier_2() -> &'static [Self] {
        &[Self::CursorIde, Self::CursorCli, Self::Opencode]
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
        let id: HarnessId = serde_json::from_str("\"gemini-cli\"").unwrap();
        assert_eq!(id, HarnessId::GeminiCli);
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
                HarnessId::GeminiCli,
                HarnessId::CodexCli,
                HarnessId::QwenCode,
            ]
        );
    }

    #[test]
    fn tier_2_contains_cursor_pair_then_opencode() {
        // Sprint 7 PR 1 landed the cursor pair; PR 2 appended Opencode.
        assert_eq!(
            HarnessId::tier_2(),
            &[
                HarnessId::CursorIde,
                HarnessId::CursorCli,
                HarnessId::Opencode,
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
            HarnessId::GeminiCli,
            HarnessId::CodexCli,
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
        ] {
            assert!(!id.label().is_empty(), "empty label for {id:?}");
        }
    }

    #[test]
    fn detection_only_harnesses_have_no_adapter() {
        for id in [
            HarnessId::JunieCli,
            HarnessId::Droid,
            HarnessId::KimiCodeCli,
            HarnessId::Devin,
            HarnessId::Forgecode,
        ] {
            assert!(!id.has_adapter(), "{id:?} should be detection-only");
        }
    }
}

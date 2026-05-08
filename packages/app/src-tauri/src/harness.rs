//! The set of AI coding harnesses Trove targets.
//!
//! Mirrors the Zod `HarnessId` enum in `packages/shared/src/schemas.ts`.
//! Variants serialize as kebab-case strings so the wire format matches
//! the TS Zod definition byte-for-byte.

use serde::{Deserialize, Serialize};

/// Discriminated union of supported AI coding harness identifiers.
/// Sprint 3 only detects + patches Tier 1 (the first four variants).
/// Tier 2 detection lands in Sprint 7; Tier 3 in Sprint 9.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HarnessId {
    ClaudeCode,
    GeminiCli,
    CodexCli,
    QwenCode,
    Opencode,
    CursorIde,
    CursorCli,
    Cline,
    Aider,
    CopilotCli,
}

impl HarnessId {
    /// Human-readable label for UI surfaces. Matches the names used in
    /// the MVP plan's harness table.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::GeminiCli => "Gemini CLI",
            Self::CodexCli => "OpenAI Codex CLI",
            Self::QwenCode => "Qwen Code",
            Self::Opencode => "OpenCode",
            Self::CursorIde => "Cursor IDE",
            Self::CursorCli => "Cursor CLI",
            Self::Cline => "Cline",
            Self::Aider => "Aider",
            Self::CopilotCli => "GitHub Copilot CLI",
        }
    }

    /// Tier 1 harnesses — those with native OTEL the configurator knows
    /// how to detect today. Sprint 3 ships detection for all four;
    /// Sprint 3 ships *adapters* for the first two only.
    #[must_use]
    pub fn tier_1() -> &'static [Self] {
        &[
            Self::ClaudeCode,
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

    /// Whether Trove currently ships an adapter for this harness — i.e.
    /// whether `apply` / `revert` will dispatch to a real implementation
    /// rather than returning `IpcError::HarnessNotImplemented`. Tier 3
    /// harnesses (`Cline`, `Aider`, `CopilotCli`) return `false` until
    /// Sprint 9 wires them up. The dashboard surfaces this as part of
    /// each `DetectedHarness` row so the UI doesn't have to maintain a
    /// parallel hard-coded list.
    #[must_use]
    pub fn has_adapter(self) -> bool {
        Self::tier_1().contains(&self) || Self::tier_2().contains(&self)
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
    fn tier_1_contains_first_four_variants_in_plan_order() {
        assert_eq!(
            HarnessId::tier_1(),
            &[
                HarnessId::ClaudeCode,
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
    fn tier_1_and_tier_2_do_not_overlap() {
        for t1 in HarnessId::tier_1() {
            assert!(
                !HarnessId::tier_2().contains(t1),
                "{t1:?} appears in both tier_1 and tier_2"
            );
        }
    }

    #[test]
    fn has_adapter_is_true_for_every_tier_1_and_tier_2_harness() {
        for id in HarnessId::tier_1().iter().chain(HarnessId::tier_2()) {
            assert!(id.has_adapter(), "{id:?} should have an adapter");
        }
    }

    #[test]
    fn has_adapter_is_false_for_tier_3_until_sprint_9() {
        for id in [HarnessId::Cline, HarnessId::Aider, HarnessId::CopilotCli] {
            assert!(
                !id.has_adapter(),
                "{id:?} should not yet have an adapter (Tier 3 lands in Sprint 9)"
            );
        }
    }

    #[test]
    fn label_is_present_for_every_variant() {
        // Exhaustive match in label() guarantees coverage; this test
        // documents that no variant returns an empty string by accident.
        for id in [
            HarnessId::ClaudeCode,
            HarnessId::GeminiCli,
            HarnessId::CodexCli,
            HarnessId::QwenCode,
            HarnessId::Opencode,
            HarnessId::CursorIde,
            HarnessId::CursorCli,
            HarnessId::Cline,
            HarnessId::Aider,
            HarnessId::CopilotCli,
        ] {
            assert!(!id.label().is_empty(), "empty label for {id:?}");
        }
    }
}

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

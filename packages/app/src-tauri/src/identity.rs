//! User-identity resolution for outgoing telemetry.
//!
//! When [`crate::app_state::Identity::enabled`] is true the collector
//! pipeline tags every signal with `user.name` and `user.email`
//! resource attributes. [`resolve`] decides the values for those tags
//! by walking a small ladder, in order:
//!
//! 1. **Per-harness probes** — each detected harness has a chance to
//!    contribute identity values from configuration it already reads.
//!    No harness exposes user identity in the files Trove reads today,
//!    so [`probe_harness`] is a stub returning `None` for every id;
//!    when a harness later starts persisting identity in its config we
//!    add the match arm in one place without expanding Trove's read
//!    surface.
//! 2. **Git config** — `git config --global user.name` /
//!    `user.email`. Most developers already have these set; using them
//!    as the auto-mode default gives the user identity tagging without
//!    re-typing values Trove has no business storing.
//! 3. **Manual override** — when
//!    [`crate::app_state::IdentitySource::Manual`] is selected, the
//!    persisted [`crate::app_state::Identity::name`] /
//!    [`crate::app_state::Identity::email`] are used verbatim
//!    regardless of the ladder above.
//!
//! [`Resolved`] returns both the final values and a label for *which*
//! source contributed them, so the UI can show "Source: detected from
//! Claude Code config" or "Source: git config" without re-running the
//! ladder.

use std::process::Command;
use std::time::Duration;

use crate::app_state::{Identity, IdentitySource};
use crate::detect::DetectedHarness;
use crate::harness::HarnessId;

/// What [`resolve`] returns alongside the resolved name/email. The UI
/// surfaces the source label so users know why their telemetry is
/// about to be tagged with a particular identity. `None` means the
/// ladder hit no signal — every layer returned empty strings.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Resolved {
    pub name: String,
    pub email: String,
    pub source: ResolvedSource,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ResolvedSource {
    /// The persisted manual override was used.
    Manual,
    /// One of the per-harness probes returned a value.
    Harness { id: HarnessId },
    /// `git config --global` provided the values.
    GitConfig,
    /// Every probe returned empty — the resource/identity processor is
    /// omitted from the collector YAML and no tagging occurs.
    None,
}

/// Resolve the identity values to use given the user's persisted
/// state and the currently-detected harnesses.
///
/// Returns `Resolved` even when the ladder hits no signal — the
/// `source` field is then [`ResolvedSource::None`]. Callers in the
/// codegen path treat `source = None` plus empty strings as "skip the
/// processor", so the collector YAML stays byte-identical to today's
/// output.
#[must_use]
pub fn resolve(identity: &Identity, harnesses: &[DetectedHarness]) -> Resolved {
    if identity.source == IdentitySource::Manual
        && (!identity.name.is_empty() || !identity.email.is_empty())
    {
        return Resolved {
            name: identity.name.clone(),
            email: identity.email.clone(),
            source: ResolvedSource::Manual,
        };
    }

    for row in harnesses.iter().filter(|h| h.detected) {
        if let Some((name, email)) = probe_harness(row) {
            if !name.is_empty() || !email.is_empty() {
                return Resolved { name, email, source: ResolvedSource::Harness { id: row.id } };
            }
        }
    }

    let git = git_config_identity();
    if !git.0.is_empty() || !git.1.is_empty() {
        return Resolved {
            name: git.0,
            email: git.1,
            source: ResolvedSource::GitConfig,
        };
    }

    // Final fallback: even with source=Auto, allow the persisted
    // values to fill in (the user may have switched from Manual to
    // Auto and we want to keep their entry rather than blanking the
    // tag). Only kicks in when nothing higher fired.
    if !identity.name.is_empty() || !identity.email.is_empty() {
        return Resolved {
            name: identity.name.clone(),
            email: identity.email.clone(),
            source: ResolvedSource::Manual,
        };
    }

    Resolved { name: String::new(), email: String::new(), source: ResolvedSource::None }
}

/// Per-harness identity probe. No harness exposes user identity in
/// the files Trove already reads today, so every arm returns `None`.
/// Future harnesses get one match arm here, plus an extension of
/// [`SECURITY.md`] §"What Trove can see" when a new file surface is
/// involved.
fn probe_harness(_row: &DetectedHarness) -> Option<(String, String)> {
    None
}

/// Read `user.name` and `user.email` from `git config --global`. Each
/// call is capped at a short timeout so a hung git config never
/// blocks a collector reload. Errors and missing values surface as
/// empty strings.
fn git_config_identity() -> (String, String) {
    let name = run_git_config("user.name").unwrap_or_default();
    let email = run_git_config("user.email").unwrap_or_default();
    (name, email)
}

fn run_git_config(key: &str) -> Option<String> {
    // 1.5s budget per call. `git config` is usually instantaneous; if
    // the user has a remote credential helper wired into a stale
    // network mount this prevents the IPC handler from hanging.
    const BUDGET: Duration = Duration::from_millis(1_500);
    let mut cmd = Command::new("git");
    cmd.args(["config", "--global", key])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    // CREATE_NO_WINDOW — git is a console app; without this flag every
    // identity probe from the GUI app flashes a console window.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd.spawn().ok()?;
    let started = std::time::Instant::now();
    loop {
        if started.elapsed() > BUDGET {
            let _ = child.kill();
            return None;
        }
        match child.try_wait().ok()? {
            Some(status) if status.success() => {
                let output = child.wait_with_output().ok()?;
                let value = String::from_utf8(output.stdout).ok()?.trim().to_string();
                if value.is_empty() {
                    return None;
                }
                return Some(value);
            }
            Some(_) => return None,
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detected(id: HarnessId) -> DetectedHarness {
        DetectedHarness {
            id,
            detected: true,
            config_path: None,
            telemetry: crate::detect::TelemetryStatus::Unknown,
            detection_method: None,
            trove_region_present: false,
            adapter_available: true,
        }
    }

    #[test]
    fn manual_override_wins_when_set() {
        let identity = Identity {
            enabled: true,
            source: IdentitySource::Manual,
            name: "Ada".into(),
            email: "ada@x".into(),
        };
        let r = resolve(&identity, &[]);
        assert_eq!(r.source, ResolvedSource::Manual);
        assert_eq!(r.name, "Ada");
        assert_eq!(r.email, "ada@x");
    }

    #[test]
    fn manual_with_empty_fields_falls_through_to_auto() {
        // User selected Manual but never typed values. The ladder
        // proceeds to git/None rather than tagging with empty strings.
        let identity = Identity {
            enabled: true,
            source: IdentitySource::Manual,
            name: String::new(),
            email: String::new(),
        };
        // No harnesses; git may or may not return values depending on
        // CI env. Source can be GitConfig or None — never Manual.
        let r = resolve(&identity, &[]);
        assert_ne!(r.source, ResolvedSource::Manual);
    }

    #[test]
    fn harness_probes_return_none_today() {
        // Stub — every harness currently returns None from
        // probe_harness. This test pins the contract: until a real
        // probe lands, no harness contributes identity.
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
            HarnessId::JunieCli,
            HarnessId::Droid,
            HarnessId::KimiCodeCli,
            HarnessId::Devin,
            HarnessId::Forgecode,
            HarnessId::Sentinel,
        ] {
            assert!(probe_harness(&detected(id)).is_none(), "harness {id:?} probe must be a stub for now");
        }
    }

    #[test]
    fn auto_with_no_signal_returns_none_source() {
        // We can't perfectly stub git here, but we can at least check
        // that Resolved is well-formed and serializable.
        let identity = Identity::default();
        let r = resolve(&identity, &[]);
        // Source is GitConfig when the host has git config set,
        // otherwise None. Either is acceptable; the name/email then
        // either reflect the host's config or are empty.
        match r.source {
            ResolvedSource::GitConfig => assert!(!r.name.is_empty() || !r.email.is_empty()),
            ResolvedSource::None => {
                assert!(r.name.is_empty());
                assert!(r.email.is_empty());
            }
            other => panic!("unexpected source {other:?} from default identity"),
        }
    }
}

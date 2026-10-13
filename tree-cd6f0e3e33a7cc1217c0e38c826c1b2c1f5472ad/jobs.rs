//! Crate-job graph model: obligations grouped by crate, never jobs per task.
//!
//! Root cause (P05): logical obligations were equated with execution
//! jobs, so per-task fan-out and vendor-prefixed internals leaked into
//! CI. The structural fix keeps obligations per task
//! ([`super::crate_job::CrateObligation`]) and groups them into one
//! ordered job per crate ([`super::crate_job::CrateJob`]): a smaller
//! job graph with identical check coverage.

use serde::{Deserialize, Serialize};

use velnor_actions_contract::errors::ContractError;

/// Generated main workflow path (P05-8: `ci.yml`, display `CI`).
pub const CI_WORKFLOW_PATH: &str = ".github/workflows/ci.yml";

/// Stale generated workflow paths the generator deletes on migration.
pub const STALE_WORKFLOW_PATHS: [&str; 1] = [".github/workflows/velnor.yml"];

/// Generated main workflow display name.
pub const WORKFLOW_DISPLAY_NAME: &str = "CI";

/// Scheduled upstream-freshness workflow path (emitted under velnor-repository-v1).
pub const FRESHNESS_WORKFLOW_PATH: &str = ".github/workflows/freshness.yml";

/// Canonical orchestration job ID for the planner.
pub const PLAN_JOB_ID: &str = "plan";

/// Canonical orchestration job ID for the required gate.
pub const REQUIRED_JOB_ID: &str = "required";

/// Display name of the planner job.
pub const PLAN_DISPLAY_NAME: &str = "Plan";

/// Display name of the required gate.
pub const REQUIRED_DISPLAY_NAME: &str = "Required";

/// Required-gate run condition: always runs, judges every conclusion.
pub const REQUIRED_CONDITION: &str = "always()";

/// Weekly upstream-freshness schedule (Mondays 06:00 UTC).
pub const FRESHNESS_CRON_WEEKLY: &str = "0 6 * * 1";

/// Display-name prefix for tofu root jobs (`OpenToFu — <root>`).
///
/// All-tofu groups take this prefix; every other group keeps
/// `Rust / `, so displays partition exactly like the ID namespaces.
pub const TOFU_DISPLAY_PREFIX: &str = "OpenToFu — ";

/// True when a display name renders safely: no expression opener, no
/// control characters (newlines included).
///
/// Job `name:` fields evaluate `${{ }}` expressions, and a control byte
/// would break YAML structure; both fail closed at the `Job` gate, and
/// constructors sanitize through `sanitize_display_text` so generated
/// names never reach the gate dirty.
#[must_use]
pub fn is_safe_display_name(name: &str) -> bool {
    !name.contains("${{") && !name.chars().any(char::is_control)
}

/// Sanitize one display label: controls become `?`, `${{` breaks open.
///
/// Cargo names can never trigger this (charset-limited); the manifest
/// folder fallback can (directory names are unconstrained), so the
/// constructor neutralizes rather than trusts. Idempotent.
fn sanitize_display_text(label: &str) -> String {
    let mut out = String::with_capacity(label.len());
    for ch in label.chars() {
        if ch.is_control() {
            out.push('?');
        } else {
            out.push(ch);
        }
    }
    out.replace("${{", "$?{{")
}

/// Display label for one crate: package name, folder fallback when missing.
#[must_use]
pub fn crate_display_label(package_name: &str, manifest: &str) -> String {
    if !package_name.trim().is_empty() {
        return sanitize_display_text(package_name);
    }
    let parent = manifest.rsplit('/').nth(1).unwrap_or_default();
    if parent.is_empty() || parent == "." {
        "workspace".to_owned()
    } else {
        sanitize_display_text(parent)
    }
}

/// Display name for one crate job: `Rust / <label>` plus config suffix.
///
/// The default configuration shows the bare label; extra feature
/// configurations append ` (<config>)` so variants never silently drop
/// checks or multiply default jobs (P05-4). The composed name is
/// sanitized whole so the configuration suffix is covered too.
#[must_use]
pub fn crate_display_name(package_name: &str, manifest: &str, configuration: &str) -> String {
    let label = crate_display_label(package_name, manifest);
    let composed = if configuration == "default" {
        format!("Rust / {label}")
    } else {
        format!("Rust / {label} ({configuration})")
    };
    sanitize_display_text(&composed)
}

/// Display name for one tofu root job: `OpenToFu — <label>`.
///
/// The label is the display root (`.` for the repository root); tofu
/// carries no configuration suffix in v1. Sanitized whole through
/// the same constructor as rust displays, so hostile labels fail
/// closed at the same gate.
#[must_use]
pub fn tofu_display_name(root_label: &str) -> String {
    sanitize_display_text(&format!("{TOFU_DISPLAY_PREFIX}{root_label}"))
}

/// Cron schedule for a generated workflow (P12-4 contract half).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleTrigger {
    /// Cron expressions (five fields each).
    pub cron: Vec<String>,
}

impl ScheduleTrigger {
    /// Validate five-field cron shape plus charset.
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.cron.is_empty() {
            return Err(ContractError::identity("schedule.cron", "empty_cron"));
        }
        for entry in &self.cron {
            let fields: Vec<&str> = entry.split_whitespace().collect();
            let shape = fields.len() == 5
                && fields.iter().all(|field| {
                    !field.is_empty()
                        && field.bytes().all(|b| {
                            b.is_ascii_alphanumeric() || matches!(b, b'*' | b'/' | b'-' | b',')
                        })
                });
            if !shape {
                return Err(ContractError::identity(
                    "schedule.cron",
                    format!("bad_cron:{entry}"),
                ));
            }
        }
        Ok(())
    }
}

/// Required-check migration from the branded gate to `Required` (P05-9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequiredCheckMigration {
    /// Old workflow path.
    pub old_workflow: String,
    /// Old required-check name.
    pub old_check: String,
    /// New workflow path.
    pub new_workflow: String,
    /// New required-check name.
    pub new_check: String,
}

impl RequiredCheckMigration {
    /// The P05 `velnor.yml` to `ci.yml` migration.
    #[must_use]
    pub fn velnor_to_ci() -> Self {
        Self {
            old_workflow: STALE_WORKFLOW_PATHS[0].to_owned(),
            old_check: "Velnor / Required".to_owned(),
            new_workflow: CI_WORKFLOW_PATH.to_owned(),
            new_check: REQUIRED_DISPLAY_NAME.to_owned(),
        }
    }

    /// Ordered migration steps; the last needs repository-admin access.
    #[must_use]
    pub fn steps(&self) -> Vec<String> {
        vec![
            format!(
                "Merge the generator change so {} replaces {} in one commit.",
                self.new_workflow, self.old_workflow
            ),
            format!(
                "Let one {} run complete on the default branch so the {} check appears.",
                self.new_workflow, self.new_check
            ),
            format!(
                "In branch protection, require {} and remove {}; never remove the old check before the new one exists.",
                self.new_check, self.old_check
            ),
        ]
    }
}

#[cfg(test)]
mod tests;

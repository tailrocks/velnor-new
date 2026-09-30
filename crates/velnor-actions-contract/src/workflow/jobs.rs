//! Crate-job graph model: obligations grouped by crate, never jobs per task.
//!
//! Root cause (P05): logical obligations were equated with execution
//! jobs, so per-task fan-out and vendor-prefixed internals leaked into
//! CI. The structural fix keeps obligations per task
//! ([`super::crate_job::CrateObligation`]) and groups them into one
//! ordered job per crate ([`super::crate_job::CrateJob`]): a smaller
//! job graph with identical check coverage.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::digest_b3;
use crate::errors::ContractError;

/// Generated main workflow path (P05-8: `ci.yml`, display `CI`).
pub const CI_WORKFLOW_PATH: &str = ".github/workflows/ci.yml";

/// Stale generated workflow paths the generator deletes on migration.
pub const STALE_WORKFLOW_PATHS: [&str; 1] = [".github/workflows/velnor.yml"];

/// Generated main workflow display name.
pub const WORKFLOW_DISPLAY_NAME: &str = "CI";

/// Scheduled upstream-freshness workflow path (renderer emits it later).
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

/// Independent validator jobs (P05-6: no Policy umbrella).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidatorKind {
    /// Repository-structure lint.
    Alint,
    /// Dependency/security audit.
    CargoDeny,
    /// Unused-dependency scan.
    CargoMachete,
    /// Workflow-file lint.
    Actionlint,
    /// Workflow security audit.
    Zizmor,
}

impl ValidatorKind {
    /// Stable unbranded job ID.
    #[must_use]
    pub fn job_id(&self) -> &'static str {
        match self {
            Self::Alint => "alint",
            Self::CargoDeny => "cargo-deny",
            Self::CargoMachete => "cargo-machete",
            Self::Actionlint => "actionlint",
            Self::Zizmor => "zizmor",
        }
    }

    /// Stable human-readable display name.
    #[must_use]
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Alint => "Alint",
            Self::CargoDeny => "Cargo Deny",
            Self::CargoMachete => "Cargo Machete",
            Self::Actionlint => "Actionlint",
            Self::Zizmor => "Zizmor",
        }
    }

    /// Every validator kind in emission order.
    #[must_use]
    pub fn all() -> [Self; 5] {
        [
            Self::Alint,
            Self::CargoDeny,
            Self::CargoMachete,
            Self::Actionlint,
            Self::Zizmor,
        ]
    }

    /// Velnor-repository validators emitted as support jobs.
    ///
    /// Actionlint is always-on base IR on both policies, never support.
    #[must_use]
    pub fn repository_validators() -> [Self; 4] {
        [
            Self::Alint,
            Self::CargoDeny,
            Self::CargoMachete,
            Self::Zizmor,
        ]
    }
}

/// Package-slug job-ID prefix: IDs below it derive from real package names.
///
/// [`assign_crate_job_ids`] emits one `rust-<slug>` ID per crate, so the
/// branding gate in [`validate_job_id`] skips this prefix: self-hosting
/// repositories keep their package names while orchestration IDs stay
/// unbranded (P05-7).
pub const CRATE_JOB_ID_PREFIX: &str = "rust-";

/// Validate a producer job ID: unbranded ASCII plus collision-safe shape.
///
/// Rejects empty IDs and non-`[a-z0-9-_]` bytes; `velnor` branding is
/// rejected on orchestration IDs only, while package-slug IDs under
/// [`CRATE_JOB_ID_PREFIX`] keep their package names so self-hosting
/// repositories validate. The legacy renderer constants keep working
/// because this gate applies to producer constructors only, never
/// `Job::validate`.
/// # Errors
pub fn validate_job_id(id: &str) -> Result<(), ContractError> {
    if id.is_empty()
        || !id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_'))
    {
        return Err(ContractError::identity(
            "job.id",
            format!("bad_job_id:{id}"),
        ));
    }
    if !id.starts_with(CRATE_JOB_ID_PREFIX) && id.contains("velnor") {
        return Err(ContractError::identity(
            "job.id",
            format!("branded_job_id:{id}"),
        ));
    }
    Ok(())
}

/// Slugify one package name into a job-ID segment.
#[must_use]
pub fn slugify_segment(name: &str) -> String {
    let mut slug = String::new();
    for byte in name.bytes() {
        let lower = byte.to_ascii_lowercase();
        let push = if lower.is_ascii_alphanumeric() {
            Some(lower)
        } else if slug.bytes().last().is_some_and(|b| b != b'-') && !slug.is_empty() {
            Some(b'-')
        } else {
            None
        };
        if let Some(byte) = push {
            slug.push(char::from(byte));
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    slug
}

/// Display label for one crate: package name, folder fallback when missing.
#[must_use]
pub fn crate_display_label(package_name: &str, manifest: &str) -> String {
    if !package_name.trim().is_empty() {
        return package_name.to_owned();
    }
    let parent = manifest.rsplit('/').nth(1).unwrap_or_default();
    if parent.is_empty() || parent == "." {
        "workspace".to_owned()
    } else {
        parent.to_owned()
    }
}

/// Display name for one crate job: `Rust / <label>` plus config suffix.
///
/// The default configuration shows the bare label; extra feature
/// configurations append ` (<config>)` so variants never silently drop
/// checks or multiply default jobs (P05-4).
#[must_use]
pub fn crate_display_name(package_name: &str, manifest: &str, configuration: &str) -> String {
    let label = crate_display_label(package_name, manifest);
    if configuration == "default" {
        format!("Rust / {label}")
    } else {
        format!("Rust / {label} ({configuration})")
    }
}

/// Assign stable collision-safe crate job IDs for one package set.
///
/// Base form is `rust-<slug>` (`rust-<slug>-<config>` off default); on
/// slug collision the later key in sorted order takes `-<digest8>` of
/// its package ID plus configuration. Deterministic for a fixed set.
#[must_use]
pub fn assign_crate_job_ids(
    crates: &BTreeSet<(String, String, String)>,
) -> BTreeMap<(String, String), String> {
    let mut assigned = BTreeMap::new();
    let mut taken = BTreeSet::new();
    for (package_id, package_name, configuration) in crates {
        let slug = slugify_segment(package_name);
        let mut base = if slug.is_empty() {
            format!("{CRATE_JOB_ID_PREFIX}workspace")
        } else {
            format!("{CRATE_JOB_ID_PREFIX}{slug}")
        };
        if configuration != "default" {
            base.push('-');
            base.push_str(&slugify_segment(configuration));
        }
        let mut id = base;
        if taken.contains(&id) {
            let digest = digest_b3(format!("{package_id}\0{configuration}").as_bytes());
            let short = digest.get(..8).unwrap_or(&digest);
            id = format!("{id}-{short}");
        }
        taken.insert(id.clone());
        assigned.insert((package_id.clone(), configuration.clone()), id);
    }
    assigned
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
mod tests {
    use super::validate_job_id;

    #[test]
    fn branding_gate_skips_package_slugs_only() {
        assert!(validate_job_id("plan").is_ok());
        assert!(validate_job_id("rust-demo").is_ok());
        assert!(validate_job_id("rust-velnor-actions-contract").is_ok());
        assert!(validate_job_id("velnor-plan").is_err());
        assert!(validate_job_id("task-velnor-final").is_err());
        assert!(validate_job_id("").is_err());
    }
}

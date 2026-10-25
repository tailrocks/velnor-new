//! Crate-job graph model: logical obligations retain one execution owner.
//!
//! The P05 migration replaced arbitrary per-task matrices and vendor-prefixed
//! jobs with package/configuration ownership and ordered obligations. The later
//! five-minute capacity amendment splits Rust into closed lint/tests/docs
//! responsibilities while retaining each logical task and its exact report.
//! A [`super::crate_job::CrateJob`] validates local obligation order and gates;
//! typed job dependencies preserve external quality/data gate owners. Required
//! must validate every emitted phase, so partitioning cannot omit coverage or
//! convert a failed, missing or canceled execution into a successful release.

use std::collections::{BTreeMap, BTreeSet};

use crate::digest_b3;
use crate::errors::ContractError;

/// Generated main workflow path (P05-8: `ci.yml`, display `CI`).
pub const CI_WORKFLOW_PATH: &str = ".github/workflows/ci.yml";

/// Generated qualification workflow path used by the qualification dispatch contract.
pub const QUALIFICATION_WORKFLOW_PATH: &str = ".github/workflows/qualification.yml";

/// Stale generated workflow paths the generator deletes on migration.
pub const STALE_WORKFLOW_PATHS: [&str; 1] = [".github/workflows/velnor.yml"];

/// Generated main workflow display name.
pub const WORKFLOW_DISPLAY_NAME: &str = "CI";

/// Scheduled upstream-freshness workflow path (emitted under velnor-repository-v1).
pub const FRESHNESS_WORKFLOW_PATH: &str = ".github/workflows/freshness.yml";

/// Protected post-merge `OpenTofu` apply workflow path.
pub const TOFU_APPLY_WORKFLOW_PATH: &str = ".github/workflows/tofu-apply.yml";

/// Canonical orchestration job ID for the planner.
pub const PLAN_JOB_ID: &str = "plan";

/// Canonical orchestration job ID for the required verdict (`merge-v1`).
pub const REQUIRED_JOB_ID: &str = "required";

/// Job ID of the branch-protection gate that reports the verdict.
pub const REQUIRED_GATE_JOB_ID: &str = "required-gate";

/// Display name of the planner job.
pub const PLAN_DISPLAY_NAME: &str = "Plan";

/// Display name of the branch-protection check, carried only by the gate
/// job, which never ends skipped: a cancelled run cannot satisfy it.
pub const REQUIRED_DISPLAY_NAME: &str = "Required";

/// Display name of the verdict job the gate reports.
pub const REQUIRED_VERDICT_DISPLAY_NAME: &str = "Required verdict";

/// Verdict run condition: runs after any needed job failed or was
/// skipped (it judges every conclusion) but not after cancellation.
///
/// `always()` also ran the full verdict after cancellation and held the
/// concurrency group for 20+ minutes. `!cancelled()` is a status function,
/// so the implicit `success()` is not added and the verdict still
/// evaluates failed or skipped needs; a cancelled run skips it.
pub const REQUIRED_CONDITION: &str = "${{ !cancelled() }}";

/// Gate run condition: always, cancellation included.
///
/// GitHub reports a skipped job's check as successful, and branch
/// protection accepts it. A cancelled run skips the verdict, so the
/// protected check must come from a job that still runs then: the gate
/// runs one runner-only step and fails unless the verdict succeeded.
pub const REQUIRED_GATE_CONDITION: &str = "${{ always() }}";

/// Gate step condition: fail whenever the verdict did not succeed.
///
/// `always()` keeps the step from inheriting the implicit `success()`,
/// which a cancelled run could turn false and leave the gate green with
/// every step skipped.
pub const REQUIRED_GATE_FAIL_CONDITION: &str =
    "${{ always() && needs.required.result != 'success' }}";

/// Weekly upstream-freshness schedule (Mondays 06:00 UTC).
pub const FRESHNESS_CRON_WEEKLY: &str = "0 6 * * 1";

mod validator_kind;
pub use validator_kind::ValidatorKind;

/// Package-slug job-ID prefix: IDs below it derive from real package names.
///
/// [`assign_crate_job_ids`] emits one `rust-<slug>` ID per rust crate,
/// so the branding gate in [`validate_job_id`] skips this prefix:
/// self-hosting repositories keep their package names while
/// orchestration IDs stay unbranded (P05-7).
pub const CRATE_JOB_ID_PREFIX: &str = "rust-";

/// Root-slug job-ID prefix: IDs below it derive from tofu root paths.
///
/// [`assign_crate_job_ids`] emits one `tofu-<slug>` ID per all-tofu
/// group, giving tofu obligations their exact-set identity without
/// touching the rust contract above. Mixed groups keep `rust-`.
pub const TOFU_JOB_ID_PREFIX: &str = "tofu-";

/// Display-name prefix for tofu root jobs (`OpenToFu — <root>`).
///
/// All-tofu groups take this prefix; every other group keeps
/// `Rust / `, so displays partition exactly like the ID namespaces.
pub const TOFU_DISPLAY_PREFIX: &str = "OpenToFu — ";

/// True for crate-group job IDs under either stack prefix.
///
/// Single definition of the crate-job ID namespace: plan counts,
/// lock/pre-seed attach, and the pre-seed closure gate all consult
/// this instead of matching one prefix.
#[must_use]
pub fn is_crate_job_id(id: &str) -> bool {
    id.starts_with(CRATE_JOB_ID_PREFIX) || id.starts_with(TOFU_JOB_ID_PREFIX)
}

/// Validate a producer job ID: unbranded ASCII plus collision-safe shape.
///
/// Rejects empty IDs and non-`[a-z0-9-_]` bytes; `velnor` branding is
/// rejected on orchestration IDs only, while crate-group IDs under
/// either stack prefix keep their real names so self-hosting
/// repositories and `velnor-*` tofu roots validate. The legacy
/// renderer constants keep working because this gate applies to
/// producer constructors only, never `Job::validate`.
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
    if !is_crate_job_id(id) && id.contains("velnor") {
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

/// Assign stable collision-safe crate job IDs for one package set.
///
/// Base form is `<prefix><slug>` (`<prefix><slug>-<config>` off
/// default); on slug collision the later key in sorted order takes
/// `-<digest8>` of its package ID plus configuration. Deterministic
/// for a fixed set. Callers pass [`CRATE_JOB_ID_PREFIX`] for rust
/// groups and [`TOFU_JOB_ID_PREFIX`] for all-tofu groups; namespaces
/// never collide across prefixes.
#[must_use]
pub fn assign_crate_job_ids(
    crates: &BTreeSet<(String, String, String)>,
    prefix: &str,
) -> BTreeMap<(String, String), String> {
    let mut assigned = BTreeMap::new();
    let mut taken = BTreeSet::new();
    for (package_id, package_name, configuration) in crates {
        let slug = slugify_segment(package_name);
        let mut base = if slug.is_empty() {
            format!("{prefix}workspace")
        } else {
            format!("{prefix}{slug}")
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

/// Give one stable ID to each Rust package for all its configurations.
#[must_use]
pub fn assign_rust_crate_job_ids(
    crates: &BTreeSet<(String, String)>,
    prefix: &str,
) -> BTreeMap<String, String> {
    let groups = crates
        .iter()
        .map(|(package_id, package_name)| {
            (
                package_id.clone(),
                package_name.clone(),
                "default".to_owned(),
            )
        })
        .collect();
    assign_crate_job_ids(&groups, prefix)
        .into_iter()
        .map(|((package_id, _), job_id)| (package_id, job_id))
        .collect()
}

#[cfg(test)]
#[path = "jobs_tests.rs"]
mod tests;

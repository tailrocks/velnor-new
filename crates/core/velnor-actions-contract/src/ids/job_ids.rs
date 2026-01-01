//! Crate-group job-ID namespace: prefixes, validation, assignment.
//!
//! Moved from `workflow::jobs` at the SIZE split: job IDs are identifier
//! vocabulary (validated and assigned here), while job display labels and
//! workflow structure stay in the workflow crate.

use std::collections::{BTreeMap, BTreeSet};

use crate::canonical::digest_b3;
use crate::errors::ContractError;

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

#[cfg(test)]
mod tests;

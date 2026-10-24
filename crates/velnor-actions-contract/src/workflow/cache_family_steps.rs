//! Generated step shapes of the stack family caches (Bun, Playwright, dbt, uv, Ansible).
//!
//! One `Compute family cache keys` step computes trust and family keys from the job's lock files.
//! This follows the `mbx-bundle` pattern.
//! Each family restores before its producer step and attempts a save immediately after it.
//! Thus, a later failing check does not prevent the save attempt (cache decision 1).
//! The family must also pass its size check before the save.

use super::cache_family::CacheFamily;

/// Id of the per-job key step.
pub const FAMILY_KEYS_ID: &str = "family-keys";
/// Key-step output: `true` only on a trusted protected push.
pub const FAMILY_SAVE_OUTPUT: &str = "save";
/// Size-step output: `true` when the directory is within the family ceiling.
pub const FAMILY_ADMITTED_OUTPUT: &str = "admitted";
/// Env of the key step in a matrix job: the leg ordinal. Only leg `1` may save,
/// so N legs of one key never race N archive uploads.
pub const FAMILY_WRITER_ENV: &str = "VELNOR_FAMILY_WRITER";
/// The only value [`FAMILY_WRITER_ENV`] takes.
pub const FAMILY_WRITER_EXPR: &str = "${{ matrix.leg }}";

/// Families whose restore, size and save steps the generator emits.
pub const STACK_FAMILIES: [CacheFamily; 5] = [
    CacheFamily::BunInstall,
    CacheFamily::PlaywrightBrowsers,
    CacheFamily::DbtPackages,
    CacheFamily::PythonVenv,
    CacheFamily::AnsibleCollections,
];

/// True for a family of [`STACK_FAMILIES`].
#[must_use]
pub fn is_stack_family(family: CacheFamily) -> bool {
    STACK_FAMILIES.contains(&family)
}

/// Output-name stem of a family (`bun_install`, `python_venv`, ...).
#[must_use]
pub fn family_output_stem(family: CacheFamily) -> String {
    family.spec().layer.replace('-', "_")
}

/// Family owning a key-step output stem.
#[must_use]
pub fn family_for_stem(stem: &str) -> Option<CacheFamily> {
    STACK_FAMILIES
        .into_iter()
        .find(|family| family_output_stem(*family) == stem)
}

/// Exact key expression of a family inside its own job.
#[must_use]
pub fn family_key_expr(family: CacheFamily) -> String {
    format!(
        "${{{{steps.{FAMILY_KEYS_ID}.outputs.{}_key}}}}",
        family_output_stem(family)
    )
}

/// Restore-keys expression (same compat, any snapshot) of a family.
#[must_use]
pub fn family_restore_keys_expr(family: CacheFamily) -> String {
    format!(
        "${{{{steps.{FAMILY_KEYS_ID}.outputs.{}_restore_keys}}}}",
        family_output_stem(family)
    )
}

/// Id of a family's size-admission step.
#[must_use]
pub fn family_size_step_id(family: CacheFamily) -> String {
    format!("family-size-{}", family.spec().layer)
}

/// Save condition: the producer succeeded (the save sits right after it), the
/// key step granted a trusted write, and the size admission passed.
#[must_use]
pub fn family_save_condition(family: CacheFamily) -> String {
    format!(
        "success() && steps.{FAMILY_KEYS_ID}.outputs.{FAMILY_SAVE_OUTPUT} == 'true' && steps.{}.outputs.{FAMILY_ADMITTED_OUTPUT} == 'true'",
        family_size_step_id(family)
    )
}

/// Convert a root path into one path segment.
/// Trim leading and trailing `/` characters for the root aliases.
/// Return `root` if the trimmed path is empty or `.`.
/// For other paths, reject an initial `/`, empty segments, and `.` or `..` segments.
/// Allow only ASCII letters, digits, `/`, `.`, `_`, and `-`.
/// Replace `/` with `-` to create one path segment.
#[must_use]
pub fn root_locator(root: &str) -> Option<String> {
    let path = root.trim_matches('/');
    if path.is_empty() || path == "." {
        return Some("root".to_owned());
    }
    let safe = root.starts_with('/')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || !path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-'));
    (!safe).then(|| path.replace('/', "-"))
}

#[cfg(test)]
#[path = "cache_family_steps_tests.rs"]
mod tests;

//! Dependency seed (`rsdeps`) wiring and the Plan cache-output budget.
//!
//! The Plan `Compute cache keys` step publishes one key and one restore
//! prefix per Cargo workspace root (deterministic root index `n`), and a
//! lookup-only probe publishes whether the exact key exists. Crate jobs
//! restore read-only from those outputs; the Cache seed workflow runs the
//! same key step and is the only writer. Keys always carry the `trusted`
//! scope: no pull-request dependency seed exists.

use sha2::{Digest, Sha256};

use super::{
    CacheFamily, PLAN_CACHE_OUTPUTS, PLAN_CACHE_TRUST_OUTPUT, ROOT_SCOPE_HEX, SOURCES_KEY_OUTPUT,
    SOURCES_RESTORE_KEYS_OUTPUT, TOFU_PROVIDERS_KEY_OUTPUT_PREFIX, tofu_providers_key_output,
};

/// Most workspace roots one workflow seeds (one static lookup id each).
pub const MAX_RSDEPS_ROOTS: usize = 16;
/// Plan output prefix: exact dependency seed key of root `n`.
pub const RSDEPS_KEY_OUTPUT_PREFIX: &str = "rsdeps_key_";
/// Plan output prefix: dependency seed restore prefix of root `n`.
pub const RSDEPS_RESTORE_KEYS_OUTPUT_PREFIX: &str = "rsdeps_restore_keys_";
/// Plan output prefix: `true` when root `n`'s exact key already exists.
pub const RSDEPS_EXACT_OUTPUT_PREFIX: &str = "rsdeps_exact_";
/// Key-step output: exact helper dependency seed key (bootstrap Plan only).
pub const RSDEPS_HELPER_KEY_OUTPUT: &str = "rsdeps_helper_key";
/// Key-step output: helper dependency seed restore prefix.
pub const RSDEPS_HELPER_RESTORE_KEYS_OUTPUT: &str = "rsdeps_helper_restore_keys";
/// Exact helper key inside a job that runs the key step.
pub const RSDEPS_HELPER_KEY_STEP_EXPR: &str = "${{steps.cache-keys.outputs.rsdeps_helper_key}}";
/// Helper restore prefix inside a job that runs the key step.
pub const RSDEPS_HELPER_RESTORE_KEYS_STEP_EXPR: &str =
    "${{steps.cache-keys.outputs.rsdeps_helper_restore_keys}}";
/// Trim-step output: the dependency seed stays at or below its ceiling.
pub const SEED_WITHIN_CEILING_OUTPUT: &str = "within_ceiling";
/// Trim-step output: the helper dependency seed stays at or below its ceiling.
pub const SEED_HELPER_WITHIN_CEILING_OUTPUT: &str = "helper_within_ceiling";

/// Save the dependency seed only as one complete generation.
///
/// Runs after any earlier failure is ruled out explicitly (never on
/// cancellation or timeout): the exact key was absent, every lane and the
/// trim succeeded, and the trimmed payload fits the configured ceiling.
pub const RSDEPS_SAVE_CONDITION: &str = "always() && !cancelled() && steps.seed-lookup.outputs.cache-hit != 'true' && steps.seed-build.outcome == 'success' && steps.seed-clippy.outcome == 'success' && steps.seed-doc.outcome == 'success' && steps.seed-trim.outcome == 'success' && steps.seed-trim.outputs.within_ceiling == 'true'";
/// Save the helper dependency seed only as one complete generation.
pub const RSDEPS_HELPER_SAVE_CONDITION: &str = "always() && !cancelled() && steps.seed-helper-lookup.outputs.cache-hit != 'true' && steps.seed-helper.outcome == 'success' && steps.seed-trim.outcome == 'success' && steps.seed-trim.outputs.helper_within_ceiling == 'true'";

/// Checkout ref of the Cache seed: the commit the triggering CI run tested.
pub const SEED_CHECKOUT_REF: &str = "${{ github.event.workflow_run.head_sha }}";

/// One sequential seed lane: a compile kind and its fixed profile directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SeedLane {
    /// Test executables (`nextest list` / `test --no-run`) in `target/debug`.
    Build,
    /// Clippy in `target/velnor-clippy/debug`.
    Clippy,
    /// Documentation dependency metadata in `target/velnor-doc/debug`.
    Doc,
    /// The bootstrap Plan helper build in `target/release`.
    Helper,
}

impl SeedLane {
    /// Lanes of the per-root dependency seed, in run order.
    pub const RSDEPS: [Self; 3] = [Self::Build, Self::Clippy, Self::Doc];

    /// Lane spelling (manifest task kind for the per-root lanes).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Build => "build",
            Self::Clippy => "clippy",
            Self::Doc => "doc",
            Self::Helper => "helper",
        }
    }

    /// Lane for a manifest task kind, when it seeds dependencies.
    #[must_use]
    pub fn for_task_kind(kind: &str) -> Option<Self> {
        Self::RSDEPS.into_iter().find(|lane| lane.as_str() == kind)
    }

    /// Family the lane's profile directory belongs to.
    #[must_use]
    pub const fn family(self) -> CacheFamily {
        match self {
            Self::Helper => CacheFamily::RsdepsHelper,
            Self::Build | Self::Clippy | Self::Doc => CacheFamily::Rsdeps,
        }
    }

    /// Workspace-relative profile directory for the root `root`.
    #[must_use]
    pub fn profile_dir(self, root: &str) -> String {
        let at = match self {
            Self::Build | Self::Helper => 0,
            Self::Clippy => 1,
            Self::Doc => 2,
        };
        self.family()
            .spec()
            .resolved_paths(Some(&root_prefix(root)))[at]
            .clone()
    }
}

/// `{root}` substitution for per-root dependency paths (`""` or `R/`).
#[must_use]
pub fn root_prefix(root: &str) -> String {
    if root.is_empty() {
        String::new()
    } else {
        format!("{root}/")
    }
}

/// `r` plus the first 12 hex of `sha256(root)`: scopes janitor deletes per root.
#[must_use]
pub fn root_scope(root: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(root.as_bytes());
    let mut scope = String::with_capacity(1 + ROOT_SCOPE_HEX);
    scope.push('r');
    for byte in digest.iter().take(ROOT_SCOPE_HEX / 2) {
        scope.push(char::from(HEX[usize::from(byte >> 4)]));
        scope.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    scope
}

/// Trusted key prefix shared by every generation of one family and root.
#[must_use]
pub fn trusted_root_prefix(family: CacheFamily, root: &str) -> String {
    format!("{}trusted-{}-", family.key_prefix(), root_scope(root))
}

/// Plan output carrying the exact dependency seed key of root `n`.
#[must_use]
pub fn rsdeps_key_output(n: usize) -> String {
    format!("{RSDEPS_KEY_OUTPUT_PREFIX}{n}")
}

/// Plan output carrying the dependency seed restore prefix of root `n`.
#[must_use]
pub fn rsdeps_restore_keys_output(n: usize) -> String {
    format!("{RSDEPS_RESTORE_KEYS_OUTPUT_PREFIX}{n}")
}

/// Plan output carrying the exact-key probe result of root `n`.
#[must_use]
pub fn rsdeps_exact_output(n: usize) -> String {
    format!("{RSDEPS_EXACT_OUTPUT_PREFIX}{n}")
}

/// `${{steps.cache-keys.outputs.<name>}}`.
#[must_use]
pub fn key_step_expr(name: &str) -> String {
    format!("${{{{steps.cache-keys.outputs.{name}}}}}")
}

/// `${{needs.plan.outputs.<name>}}`.
#[must_use]
pub fn plan_output_expr(name: &str) -> String {
    format!("${{{{needs.plan.outputs.{name}}}}}")
}

/// Ordinal of a `<prefix><n>` output name, when well formed.
#[must_use]
pub fn output_ordinal(name: &str, prefix: &str) -> Option<usize> {
    name.strip_prefix(prefix)
        .filter(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|digits| digits.parse().ok())
}

/// True for a Plan output promoted from the key step or a seed lookup.
#[must_use]
pub fn is_plan_cache_output(name: &str) -> bool {
    PLAN_CACHE_OUTPUTS.iter().any(|(output, _)| *output == name)
        || output_ordinal(name, TOFU_PROVIDERS_KEY_OUTPUT_PREFIX).is_some()
        || rsdeps_output_ordinal(name).is_some()
}

/// Root ordinal of an `rsdeps_{key,restore_keys,exact}_<n>` output.
#[must_use]
pub fn rsdeps_output_ordinal(name: &str) -> Option<usize> {
    [
        RSDEPS_KEY_OUTPUT_PREFIX,
        RSDEPS_RESTORE_KEYS_OUTPUT_PREFIX,
        RSDEPS_EXACT_OUTPUT_PREFIX,
    ]
    .into_iter()
    .find_map(|prefix| output_ordinal(name, prefix))
    .filter(|ordinal| *ordinal < MAX_RSDEPS_ROOTS)
}

/// UTF-16 bytes reserved out of the Plan job output budget for cache outputs.
pub const PLAN_CACHE_OUTPUTS_RESERVE_UTF16_BYTES: usize = 64 * 1024;

/// Conservative UTF-16 bytes the key-step outputs add to the Plan job outputs.
///
/// Generation fails when this exceeds [`PLAN_CACHE_OUTPUTS_RESERVE_UTF16_BYTES`],
/// which the plan step keeps free, so cache keys never push the promoted
/// outputs past GitHub's per-job limit.
#[must_use]
pub fn cache_output_reserve_utf16_bytes(tofu_roots: usize, rsdeps_roots: usize) -> usize {
    let record = |name: &str, value: usize| (name.len() + 1 + value + 1) * 2;
    let sources = CacheFamily::Sources.max_key_bytes();
    let tofu = CacheFamily::TofuProviders.max_key_bytes();
    let rsdeps = CacheFamily::Rsdeps.max_key_bytes();
    let last = rsdeps_roots.max(1) - 1;
    record(PLAN_CACHE_TRUST_OUTPUT, 7)
        + record(SOURCES_KEY_OUTPUT, sources)
        + record(SOURCES_RESTORE_KEYS_OUTPUT, 2 * sources + 1)
        + tofu_roots * record(&tofu_providers_key_output(tofu_roots), tofu)
        + rsdeps_roots
            * (record(&rsdeps_key_output(last), rsdeps)
                + record(&rsdeps_restore_keys_output(last), rsdeps)
                + record(&rsdeps_exact_output(last), 5))
}

#[cfg(test)]
#[path = "cache_family_seed_tests.rs"]
mod tests;

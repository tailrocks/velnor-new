//! Each cache family has one key layer, schema, and path list.
//!
//! Restore and save steps use [`FamilySpec::path_input`].
//! Both steps use the same path list.
//! The Plan job computes run-time keys with the generated
//! `Compute cache keys` step.
//! Each key uses `velnor-v<schema>-<layer>-<trust>-<compat>-<snapshot>`.
//! Each digest uses `s256-<64 hex>` form.
//! The step uses `sha256sum` to make the digests.
//! Runner images provide `sha256sum`. They do not provide a BLAKE3 tool.
//! Tests pin each schema and path list.
//! Increase the schema when a path list changes.

use crate::cachekey::MAX_CACHE_KEY_BYTES;
use crate::errors::ContractError;

/// Owned Cargo home expression shared by every sources restore and save.
pub const CARGO_HOME_EXPR: &str = "${{ runner.temp }}/velnor/cargo";
/// Runner-temp expression spelling inside family path templates.
pub const RUNNER_TEMP_EXPR: &str = "${{ runner.temp }}";
/// Shell spelling of [`RUNNER_TEMP_EXPR`] inside the key step.
pub const RUNNER_TEMP_SHELL: &str = "$RUNNER_TEMP";
/// Placeholder for the opaque per-root locator in a per-root family path.
pub const ROOT_PLACEHOLDER: &str = "{root}";
/// Archived Cargo-home subset, relative to [`CARGO_HOME_EXPR`].
pub const SOURCE_SUBSET: [&str; 3] = ["registry/index", "registry/cache", "git/db"];
/// Base of the job-private `OpenTofu` plugin caches.
pub const TOFU_PROVIDER_CACHE_BASE_EXPR: &str = "${{ runner.temp }}/velnor/tofu-cache";

const SOURCES_PATHS: [&str; 3] = [
    "${{ runner.temp }}/velnor/cargo/registry/index",
    "${{ runner.temp }}/velnor/cargo/registry/cache",
    "${{ runner.temp }}/velnor/cargo/git/db",
];
const TOFU_PROVIDERS_PATHS: [&str; 1] = ["${{ runner.temp }}/velnor/tofu-cache/{root}"];

/// One runtime-keyed cache family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CacheFamily {
    /// Cargo registry index, registry cache, and Git database.
    Sources,
    /// One `OpenTofu` root's job-private provider plugin cache.
    TofuProviders,
}

/// Static identity of one family: key layer, schema, and archive paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FamilySpec {
    /// Key layer segment (`velnor-v<schema>-<layer>-`).
    pub layer: &'static str,
    /// Family schema; bumped whenever paths or digest inputs change.
    pub schema: u32,
    /// Archive paths in expression form, in declared order.
    pub paths: &'static [&'static str],
}

const SOURCES: FamilySpec = FamilySpec {
    layer: "sources",
    schema: 2,
    paths: &SOURCES_PATHS,
};
const TOFU_PROVIDERS: FamilySpec = FamilySpec {
    layer: "tofu-providers",
    schema: 2,
    paths: &TOFU_PROVIDERS_PATHS,
};

impl CacheFamily {
    /// Every family, in key-prefix order.
    pub const ALL: [Self; 2] = [Self::Sources, Self::TofuProviders];

    /// Static family identity.
    #[must_use]
    pub const fn spec(self) -> &'static FamilySpec {
        match self {
            Self::Sources => &SOURCES,
            Self::TofuProviders => &TOFU_PROVIDERS,
        }
    }

    /// Key prefix shared by every entry of this family.
    #[must_use]
    pub fn key_prefix(self) -> String {
        let spec = self.spec();
        format!("velnor-v{}-{}-", spec.schema, spec.layer)
    }

    /// Family owning `key`, by exact prefix.
    #[must_use]
    pub fn for_key(key: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|family| key.starts_with(&family.key_prefix()))
    }

    /// Longest key the runtime step can emit for this family.
    #[must_use]
    pub fn max_key_bytes(self) -> usize {
        let trust = CacheTrust::ALL
            .iter()
            .map(|trust| trust.as_str().len())
            .max()
            .unwrap_or(0);
        self.key_prefix().len() + trust + 1 + S256_DIGEST_BYTES + 1 + S256_DIGEST_BYTES
    }

    /// Fail generation when this family's longest key exceeds GitHub's cap.
    /// # Errors
    ///
    /// Returns a contract error naming the family when its bound is too long.
    pub fn validate_key_bound(self) -> Result<(), ContractError> {
        if self.max_key_bytes() > MAX_CACHE_KEY_BYTES {
            return Err(ContractError::identity(
                "cache.key",
                format!("key_too_long:{}", self.spec().layer),
            ));
        }
        Ok(())
    }
}

impl FamilySpec {
    /// Paths with the per-root locator substituted (none for shared families).
    #[must_use]
    pub fn resolved_paths(&self, root_locator: Option<&str>) -> Vec<String> {
        self.paths
            .iter()
            .map(|path| match root_locator {
                Some(locator) => path.replace(ROOT_PLACEHOLDER, locator),
                None => (*path).to_owned(),
            })
            .collect()
    }

    /// The one `path:` input value every restore and save of this family uses.
    #[must_use]
    pub fn path_input(&self, root_locator: Option<&str>) -> String {
        self.resolved_paths(root_locator).join("\n")
    }

    /// Paths spelled for the key step's shell (`$RUNNER_TEMP`).
    #[must_use]
    pub fn shell_paths(&self, root_locator: Option<&str>) -> Vec<String> {
        self.resolved_paths(root_locator)
            .into_iter()
            .map(|path| path.replace(RUNNER_TEMP_EXPR, RUNNER_TEMP_SHELL))
            .collect()
    }

    /// True when the family archives one path set per root.
    #[must_use]
    pub fn is_per_root(&self) -> bool {
        self.paths
            .iter()
            .any(|path| path.contains(ROOT_PLACEHOLDER))
    }
}

/// Pin digest over one spec's schema and path templates.
///
/// Tests pin each family's `(schema, paths)` pair to a constant, so a path
/// edit without a schema bump fails instead of silently colliding with
/// entries written under the old path list.
#[must_use]
pub fn spec_pin_digest(spec: &FamilySpec) -> String {
    let mut bytes = format!("layer={}\nschema={}\n", spec.layer, spec.schema);
    for path in spec.paths {
        bytes.push_str(path);
        bytes.push('\n');
    }
    blake3::hash(bytes.as_bytes()).to_hex().to_string()
}

/// Runtime cache trust computed by the key step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheTrust {
    /// Protected push to the default branch.
    Trusted,
    /// Same-repository pull request (PR-scoped writes only).
    Pr,
    /// Forks, merge groups, dispatches, and every other push: never writes.
    None,
}

impl CacheTrust {
    /// Every trust value.
    pub const ALL: [Self; 3] = [Self::Trusted, Self::Pr, Self::None];

    /// Output spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Trusted => "trusted",
            Self::Pr => "pr",
            Self::None => "none",
        }
    }
}

/// `s256-` plus 64 lowercase hex characters.
pub const S256_DIGEST_BYTES: usize = 69;
/// Display name of the generated key step.
pub const CACHE_KEYS_STEP_NAME: &str = "Compute cache keys";
/// Key-step output: runtime trust.
pub const TRUST_OUTPUT: &str = "trust";
/// Key-step output: exact sources key.
pub const SOURCES_KEY_OUTPUT: &str = "sources_key";
/// Key-step output: newline-separated sources restore keys.
pub const SOURCES_RESTORE_KEYS_OUTPUT: &str = "sources_restore_keys";
/// Key-step output: `true` only when this run may write sources.
pub const SOURCES_SAVE_OUTPUT: &str = "sources_save";
/// Key-step output: sources snapshot digest (exact-content marker).
pub const SOURCES_SNAPSHOT_OUTPUT: &str = "sources_snapshot";
/// Plan job output carrying the runtime trust.
pub const PLAN_CACHE_TRUST_OUTPUT: &str = "cache_trust";
/// Plan job output prefix for per-root provider keys (`<prefix><ordinal>`).
pub const TOFU_PROVIDERS_KEY_OUTPUT_PREFIX: &str = "tofu_providers_key_";

/// Exact sources key inside the Plan job (no spaces: cache keys reject them).
pub const SOURCES_KEY_STEP_EXPR: &str = "${{steps.cache-keys.outputs.sources_key}}";
/// Sources restore keys inside the Plan job.
pub const SOURCES_RESTORE_KEYS_STEP_EXPR: &str =
    "${{steps.cache-keys.outputs.sources_restore_keys}}";
/// Exact sources key in jobs that need the Plan job.
pub const SOURCES_KEY_PLAN_EXPR: &str = "${{needs.plan.outputs.sources_key}}";
/// Sources restore keys in jobs that need the Plan job.
pub const SOURCES_RESTORE_KEYS_PLAN_EXPR: &str = "${{needs.plan.outputs.sources_restore_keys}}";

/// Producer-gated Plan-only sources save.
///
/// Runs after unrelated later failures (`!cancelled()`), but only when the
/// last fetch step succeeded (fetch steps are unconditional and sequential,
/// so the last one succeeds only when every one did), the key step granted
/// writes (trusted or same-repository PR), and the restored entry does not
/// already hold this exact snapshot.
pub const SOURCES_SAVE_CONDITION: &str = "always() && !cancelled() && steps.sources-fetch.outcome == 'success' && steps.cache-keys.outputs.sources_save == 'true' && steps.sources-restore.outputs.cache-hit != 'true' && !endsWith(steps.sources-restore.outputs.cache-matched-key, steps.cache-keys.outputs.sources_snapshot)";

/// Trusted-only provider save in the root's own job.
pub const TOFU_PROVIDERS_SAVE_CONDITION: &str =
    "success() && needs.plan.outputs.cache_trust == 'trusted'";

/// Job output name and key-step output for each shared Plan cache output.
pub const PLAN_CACHE_OUTPUTS: [(&str, &str); 3] = [
    (PLAN_CACHE_TRUST_OUTPUT, TRUST_OUTPUT),
    (SOURCES_KEY_OUTPUT, SOURCES_KEY_OUTPUT),
    (SOURCES_RESTORE_KEYS_OUTPUT, SOURCES_RESTORE_KEYS_OUTPUT),
];

/// Plan job output name for the provider key of tofu root `ordinal`.
#[must_use]
pub fn tofu_providers_key_output(ordinal: usize) -> String {
    format!("{TOFU_PROVIDERS_KEY_OUTPUT_PREFIX}{ordinal}")
}

/// Provider key expression for tofu root `ordinal` in jobs that need Plan.
#[must_use]
pub fn tofu_providers_key_plan_expr(ordinal: usize) -> String {
    format!(
        "${{{{needs.plan.outputs.{}}}}}",
        tofu_providers_key_output(ordinal)
    )
}

/// True for a Plan output promoted from the key step.
#[must_use]
pub fn is_plan_cache_output(name: &str) -> bool {
    PLAN_CACHE_OUTPUTS.iter().any(|(output, _)| *output == name)
        || name
            .strip_prefix(TOFU_PROVIDERS_KEY_OUTPUT_PREFIX)
            .is_some_and(|ordinal| {
                !ordinal.is_empty() && ordinal.bytes().all(|b| b.is_ascii_digit())
            })
}

/// UTF-16 bytes reserved out of the Plan job output budget for cache outputs.
pub const PLAN_CACHE_OUTPUTS_RESERVE_UTF16_BYTES: usize = 64 * 1024;

/// Maximum UTF-16 bytes added by the key-step outputs.
///
/// Generation fails when this exceeds [`PLAN_CACHE_OUTPUTS_RESERVE_UTF16_BYTES`].
/// The Plan step keeps this space free.
/// Cache keys then stay within GitHub's per-job output limit.
#[must_use]
pub fn cache_output_reserve_utf16_bytes(tofu_roots: usize) -> usize {
    let record = |name: &str, value: usize| (name.len() + 1 + value + 1) * 2;
    let sources = CacheFamily::Sources.max_key_bytes();
    let tofu = CacheFamily::TofuProviders.max_key_bytes();
    record(PLAN_CACHE_TRUST_OUTPUT, 7)
        + record(SOURCES_KEY_OUTPUT, sources)
        + record(SOURCES_RESTORE_KEYS_OUTPUT, 2 * sources + 1)
        + tofu_roots * record(&tofu_providers_key_output(tofu_roots), tofu)
}

#[cfg(test)]
#[path = "cache_family_tests.rs"]
mod tests;

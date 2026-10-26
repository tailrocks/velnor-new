// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::cachekey::MAX_CACHE_KEY_BYTES;
use crate::errors::ContractError;

pub const CARGO_HOME_EXPR: &str = "${{ runner.temp }}/velnor/cargo";

pub const RUNNER_TEMP_EXPR: &str = "${{ runner.temp }}";

pub const RUNNER_TEMP_SHELL: &str = "$RUNNER_TEMP";

pub const ROOT_PLACEHOLDER: &str = "{root}";

pub const SOURCE_SUBSET: [&str; 3] = ["registry/index", "registry/cache", "git/db"];

pub const TOFU_PROVIDER_CACHE_BASE_EXPR: &str = "${{ runner.temp }}/velnor/tofu-cache";

const SOURCES_PATHS: [&str; 3] = [
    "${{ runner.temp }}/velnor/cargo/registry/index",
    "${{ runner.temp }}/velnor/cargo/registry/cache",
    "${{ runner.temp }}/velnor/cargo/git/db",
];
const TOFU_PROVIDERS_PATHS: [&str; 1] = ["${{ runner.temp }}/velnor/tofu-cache/{root}"];
const BUN_INSTALL_PATHS: [&str; 1] = ["${{ runner.temp }}/velnor/bun-cache"];
const PLAYWRIGHT_BROWSERS_PATHS: [&str; 1] = ["${{ runner.temp }}/velnor/ms-playwright"];
const DBT_PACKAGES_PATHS: [&str; 1] = ["${{ runner.temp }}/velnor/dbt-packages/{root}"];
const PYTHON_VENV_PATHS: [&str; 1] = ["${{ runner.temp }}/velnor/uv-cache"];
const ANSIBLE_COLLECTIONS_PATHS: [&str; 1] = ["${{ runner.temp }}/velnor/ansible-collections"];
use super::cache_family_mbx::MBX_BUNDLE_PATH_EXPR;
const MBX_BUNDLE_PATHS: [&str; 1] = [MBX_BUNDLE_PATH_EXPR];

pub const CACHE_QUOTA_BYTES: u64 = 10 * 1024 * 1024 * 1024;

pub const CACHE_BUDGET_PERCENT: u64 = 85;
const MIB: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CacheFamily {
    Sources,

    TofuProviders,

    BunInstall,

    PlaywrightBrowsers,

    DbtPackages,

    PythonVenv,

    AnsibleCollections,

    MbxBundle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FamilySpec {
    pub layer: &'static str,

    pub schema: u32,

    pub paths: &'static [&'static str],

    pub ceiling_bytes: u64,

    pub retained_entries: u64,
}

const SOURCES: FamilySpec = FamilySpec {
    layer: "sources",
    schema: 2,
    paths: &SOURCES_PATHS,
    ceiling_bytes: 128 * MIB,
    retained_entries: 3,
};
const TOFU_PROVIDERS: FamilySpec = FamilySpec {
    layer: "tofu-providers",
    schema: 2,
    paths: &TOFU_PROVIDERS_PATHS,
    ceiling_bytes: 256 * MIB,
    retained_entries: 3,
};
const BUN_INSTALL: FamilySpec = FamilySpec {
    layer: "bun-install",
    schema: 1,
    paths: &BUN_INSTALL_PATHS,
    ceiling_bytes: 256 * MIB,
    retained_entries: 2,
};
const PLAYWRIGHT_BROWSERS: FamilySpec = FamilySpec {
    layer: "playwright-browsers",
    schema: 1,
    paths: &PLAYWRIGHT_BROWSERS_PATHS,
    ceiling_bytes: 512 * MIB,
    retained_entries: 2,
};

const DBT_PACKAGES: FamilySpec = FamilySpec {
    layer: "dbt-packages",
    schema: 1,
    paths: &DBT_PACKAGES_PATHS,
    ceiling_bytes: 64 * MIB,
    retained_entries: 2,
};
const PYTHON_VENV: FamilySpec = FamilySpec {
    layer: "python-venv",
    schema: 1,
    paths: &PYTHON_VENV_PATHS,
    ceiling_bytes: 384 * MIB,
    retained_entries: 2,
};
const ANSIBLE_COLLECTIONS: FamilySpec = FamilySpec {
    layer: "ansible-collections",
    schema: 1,
    paths: &ANSIBLE_COLLECTIONS_PATHS,
    ceiling_bytes: 128 * MIB,
    retained_entries: 2,
};

const MBX_BUNDLE: FamilySpec = FamilySpec {
    layer: "mbx-bundle",
    schema: 1,
    paths: &MBX_BUNDLE_PATHS,
    ceiling_bytes: 384 * MIB,
    retained_entries: 1,
};

impl CacheFamily {
    pub const ALL: [Self; 8] = [
        Self::Sources,
        Self::TofuProviders,
        Self::BunInstall,
        Self::PlaywrightBrowsers,
        Self::DbtPackages,
        Self::PythonVenv,
        Self::AnsibleCollections,
        Self::MbxBundle,
    ];

    #[must_use]
    pub const fn spec(self) -> &'static FamilySpec {
        match self {
            Self::Sources => &SOURCES,
            Self::TofuProviders => &TOFU_PROVIDERS,
            Self::BunInstall => &BUN_INSTALL,
            Self::PlaywrightBrowsers => &PLAYWRIGHT_BROWSERS,
            Self::DbtPackages => &DBT_PACKAGES,
            Self::PythonVenv => &PYTHON_VENV,
            Self::AnsibleCollections => &ANSIBLE_COLLECTIONS,
            Self::MbxBundle => &MBX_BUNDLE,
        }
    }

    #[must_use]
    pub fn key_prefix(self) -> String {
        let spec = self.spec();
        format!("velnor-v{}-{}-", spec.schema, spec.layer)
    }

    #[must_use]
    pub fn for_key(key: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|family| key.starts_with(&family.key_prefix()))
    }

    #[must_use]
    pub fn max_key_bytes(self) -> usize {
        let trust = CacheTrust::ALL
            .iter()
            .map(|trust| trust.as_str().len())
            .max()
            .unwrap_or(0);
        self.key_prefix().len() + trust + 1 + S256_DIGEST_BYTES + 1 + S256_DIGEST_BYTES
    }

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

    #[must_use]
    pub fn path_input(&self, root_locator: Option<&str>) -> String {
        self.resolved_paths(root_locator).join("\n")
    }

    #[must_use]
    pub fn shell_paths(&self, root_locator: Option<&str>) -> Vec<String> {
        self.resolved_paths(root_locator)
            .into_iter()
            .map(|path| path.replace(RUNNER_TEMP_EXPR, RUNNER_TEMP_SHELL))
            .collect()
    }

    #[must_use]
    pub fn is_per_root(&self) -> bool {
        self.paths
            .iter()
            .any(|path| path.contains(ROOT_PLACEHOLDER))
    }
}

impl FamilySpec {
    #[must_use]
    pub const fn worst_case_bytes(&self, units: u64) -> u64 {
        self.ceiling_bytes * self.retained_entries * units
    }
}

#[must_use]
pub fn spec_pin_digest(spec: &FamilySpec) -> String {
    let mut bytes = format!("layer={}\nschema={}\n", spec.layer, spec.schema);
    for path in spec.paths {
        bytes.push_str(path);
        bytes.push('\n');
    }
    blake3::hash(bytes.as_bytes()).to_hex().to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheTrust {
    Trusted,

    Pr,

    None,
}

impl CacheTrust {
    pub const ALL: [Self; 3] = [Self::Trusted, Self::Pr, Self::None];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Trusted => "trusted",
            Self::Pr => "pr",
            Self::None => "none",
        }
    }
}

pub const S256_DIGEST_BYTES: usize = 69;

pub const CACHE_KEYS_STEP_NAME: &str = "Compute cache keys";

pub const TRUST_OUTPUT: &str = "trust";

pub const SOURCES_KEY_OUTPUT: &str = "sources_key";

pub const SOURCES_RESTORE_KEYS_OUTPUT: &str = "sources_restore_keys";

pub const SOURCES_SAVE_OUTPUT: &str = "sources_save";

pub const SOURCES_SNAPSHOT_OUTPUT: &str = "sources_snapshot";

pub const PLAN_CACHE_TRUST_OUTPUT: &str = "cache_trust";

pub const TOFU_PROVIDERS_KEY_OUTPUT_PREFIX: &str = "tofu_providers_key_";

pub const SOURCES_KEY_STEP_EXPR: &str = "${{steps.cache-keys.outputs.sources_key}}";

pub const SOURCES_RESTORE_KEYS_STEP_EXPR: &str =
    "${{steps.cache-keys.outputs.sources_restore_keys}}";

pub const SOURCES_KEY_PLAN_EXPR: &str = "${{needs.plan.outputs.sources_key}}";

pub const SOURCES_RESTORE_KEYS_PLAN_EXPR: &str = "${{needs.plan.outputs.sources_restore_keys}}";

pub const SOURCES_SAVE_CONDITION: &str = "always() && !cancelled() && steps.sources-fetch.outcome == 'success' && steps.cache-keys.outputs.sources_save == 'true' && steps.sources-restore.outputs.cache-hit != 'true' && !endsWith(steps.sources-restore.outputs.cache-matched-key, steps.cache-keys.outputs.sources_snapshot)";

pub const TOFU_PROVIDERS_SAVE_CONDITION: &str =
    "success() && needs.plan.outputs.cache_trust == 'trusted'";

pub const PLAN_CACHE_OUTPUTS: [(&str, &str); 3] = [
    (PLAN_CACHE_TRUST_OUTPUT, TRUST_OUTPUT),
    (SOURCES_KEY_OUTPUT, SOURCES_KEY_OUTPUT),
    (SOURCES_RESTORE_KEYS_OUTPUT, SOURCES_RESTORE_KEYS_OUTPUT),
];

#[must_use]
pub fn tofu_providers_key_output(ordinal: usize) -> String {
    format!("{TOFU_PROVIDERS_KEY_OUTPUT_PREFIX}{ordinal}")
}

#[must_use]
pub fn tofu_providers_key_plan_expr(ordinal: usize) -> String {
    format!(
        "${{{{needs.plan.outputs.{}}}}}",
        tofu_providers_key_output(ordinal)
    )
}

#[must_use]
pub fn is_plan_cache_output(name: &str) -> bool {
    PLAN_CACHE_OUTPUTS.iter().any(|(output, _)| *output == name)
        || name
            .strip_prefix(TOFU_PROVIDERS_KEY_OUTPUT_PREFIX)
            .is_some_and(|ordinal| {
                !ordinal.is_empty() && ordinal.bytes().all(|b| b.is_ascii_digit())
            })
}

pub const PLAN_CACHE_OUTPUTS_RESERVE_UTF16_BYTES: usize = 64 * 1024;

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

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::cachekey::MAX_CACHE_KEY_BYTES;
use crate::errors::ContractError;
use crate::workflow::cache_identity::Gc1Family;

use super::{CacheTrust, S256_DIGEST_BYTES};

pub const CARGO_HOME_EXPR: &str = "${{ runner.temp }}/velnor/cargo";

pub const RUSTUP_HOME_EXPR: &str = "${{ runner.temp }}/velnor/rustup";

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
pub const COMPLETE_STATE_ROOT_EXPR: &str = "${{ runner.temp }}/velnor-gc1/state";

pub const COMPLETE_STATE_TRANSPORT_EXPR: &str =
    "${{ runner.temp }}/velnor-gc1/transport/state/payload.tar.zst";

const STATE_PATHS: [&str; 1] = [COMPLETE_STATE_ROOT_EXPR];

pub const GC1_HARD_CEILING_BYTES: u64 = 10_000_000_000;

pub const GC1_MAX_GENERATION_SLOTS: u64 = 2;

pub(super) const MIB: u64 = 1024 * 1024;

pub(super) const GIB: u64 = 1024 * MIB;

pub const TOOLS_LAYER_ALLOWANCE_BYTES: u64 = GIB + GIB / 2;

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
pub enum Ceiling {
    Single(u64),

    StoredAndExtracted { stored: u64, extracted: u64 },
}

impl Ceiling {
    #[must_use]
    pub const fn stored(self) -> u64 {
        match self {
            Self::Single(bytes) | Self::StoredAndExtracted { stored: bytes, .. } => bytes,
        }
    }

    #[must_use]
    pub const fn extracted(self) -> u64 {
        match self {
            Self::Single(bytes)
            | Self::StoredAndExtracted {
                extracted: bytes, ..
            } => bytes,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FamilySpec {
    pub layer: &'static str,

    pub schema: u32,

    pub paths: &'static [&'static str],

    pub ceiling: Ceiling,

    pub retained_entries: u64,
}

const SOURCES: FamilySpec = FamilySpec {
    layer: "sources",
    schema: 2,
    paths: &SOURCES_PATHS,
    ceiling: Ceiling::Single(128 * MIB),
    retained_entries: 2,
};
const TOFU_PROVIDERS: FamilySpec = FamilySpec {
    layer: "tofu-providers",
    schema: 2,
    paths: &TOFU_PROVIDERS_PATHS,
    ceiling: Ceiling::Single(256 * MIB),
    retained_entries: 2,
};
const BUN_INSTALL: FamilySpec = FamilySpec {
    layer: "bun-install",
    schema: 1,
    paths: &BUN_INSTALL_PATHS,
    ceiling: Ceiling::Single(256 * MIB),
    retained_entries: 2,
};
const PLAYWRIGHT_BROWSERS: FamilySpec = FamilySpec {
    layer: "playwright-browsers",
    schema: 1,
    paths: &PLAYWRIGHT_BROWSERS_PATHS,
    ceiling: Ceiling::Single(512 * MIB),
    retained_entries: 2,
};

const DBT_PACKAGES: FamilySpec = FamilySpec {
    layer: "dbt-packages",
    schema: 1,
    paths: &DBT_PACKAGES_PATHS,
    ceiling: Ceiling::Single(64 * MIB),
    retained_entries: 2,
};
const PYTHON_VENV: FamilySpec = FamilySpec {
    layer: "python-venv",
    schema: 1,
    paths: &PYTHON_VENV_PATHS,
    ceiling: Ceiling::Single(384 * MIB),
    retained_entries: 2,
};
const ANSIBLE_COLLECTIONS: FamilySpec = FamilySpec {
    layer: "ansible-collections",
    schema: 1,
    paths: &ANSIBLE_COLLECTIONS_PATHS,
    ceiling: Ceiling::Single(128 * MIB),
    retained_entries: 2,
};

const MBX_BUNDLE: FamilySpec = FamilySpec {
    layer: "state",
    schema: 5,
    paths: &STATE_PATHS,
    ceiling: Ceiling::StoredAndExtracted {
        stored: GIB + GIB / 2,
        extracted: 12 * GIB,
    },
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
        match self {
            Self::MbxBundle => super::state::widest_entry_key_bytes(),
            _ => self.key_prefix().len() + trust + 1 + S256_DIGEST_BYTES + 1 + S256_DIGEST_BYTES,
        }
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

    #[must_use]
    pub fn gc1_family(self) -> Option<Gc1Family> {
        match self {
            Self::Sources => Some(Gc1Family::Sources),
            Self::MbxBundle => Some(Gc1Family::State),
            Self::TofuProviders
            | Self::BunInstall
            | Self::PlaywrightBrowsers
            | Self::DbtPackages
            | Self::PythonVenv
            | Self::AnsibleCollections => None,
        }
    }
}

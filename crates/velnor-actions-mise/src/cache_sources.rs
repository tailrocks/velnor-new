// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::MiseError;
use velnor_actions_contract::StepRole;

pub use velnor_actions_contract::workflow::cache_family::SOURCE_SUBSET;

pub const TRUSTED_WRITER_ROLE: &str = "plan";

pub const NEVER_ARCHIVE_MARKERS: [&str; 3] = ["credentials", ".tfstate", ".tfplan"];

#[must_use]
pub fn is_never_archive_path(path: &str) -> bool {
    NEVER_ARCHIVE_MARKERS
        .iter()
        .any(|marker| path.contains(marker))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchDecision {
    OfflineSkip,
    ExplicitFetch { miss_reason: &'static str },
}

pub fn sources_cache_paths(cargo_home: &str) -> Result<Vec<String>, MiseError> {
    if cargo_home.trim().is_empty() {
        return Err(MiseError::InvalidStepInput {
            field: "cargo_home".to_owned(),
            value: cargo_home.to_owned(),
        });
    }
    Ok(SOURCE_SUBSET
        .iter()
        .map(|suffix| format!("{cargo_home}/{suffix}"))
        .collect())
}

pub fn validate_sources_subset(paths: &[String], cargo_home: &str) -> Result<(), MiseError> {
    for path in paths {
        if path.contains("..") || is_never_archive_path(path) {
            return Err(reject(path));
        }
        if !path.starts_with(&format!("{cargo_home}/")) {
            return Err(reject(path));
        }
        let suffix = path.strip_prefix(&format!("{cargo_home}/")).unwrap_or("");
        if suffix.starts_with("registry/src") {
            return Err(reject(path));
        }
        let allowed = SOURCE_SUBSET
            .iter()
            .any(|ok| suffix == *ok || suffix.starts_with(&format!("{ok}/")));
        if !allowed {
            return Err(reject(path));
        }
    }
    Ok(())
}

fn reject(path: &str) -> MiseError {
    MiseError::InvalidStepInput {
        field: "sources_path".to_owned(),
        value: path.to_owned(),
    }
}

#[must_use]
pub fn is_trusted_writer(role: &str) -> bool {
    role == TRUSTED_WRITER_ROLE
}

pub fn fetch_decision(
    sources_complete: bool,
    miss_reason: &'static str,
) -> Result<FetchDecision, MiseError> {
    if sources_complete {
        return Ok(FetchDecision::OfflineSkip);
    }
    if !matches!(
        miss_reason,
        "no_entry" | "source_missing" | "cache_unavailable" | "cache_corrupt"
    ) {
        return Err(MiseError::InvalidStepInput {
            field: "miss_reason".to_owned(),
            value: miss_reason.to_owned(),
        });
    }
    Ok(FetchDecision::ExplicitFetch { miss_reason })
}

pub fn check_restore_before_fetch(
    roles: &[Option<StepRole>],
    has_mbx: bool,
) -> Result<(), MiseError> {
    let fetches = roles
        .iter()
        .enumerate()
        .filter_map(|(index, role)| (*role == Some(StepRole::CargoSourcesFetch)).then_some(index));
    for fetch_at in fetches {
        let source_restore = roles
            .iter()
            .position(|role| *role == Some(StepRole::CargoSourcesRestore));
        if source_restore.is_some_and(|restore_at| fetch_at < restore_at) {
            return Err(MiseError::CacheNotEligible {
                task: "fetch".to_owned(),
                reason: "fetch_before_restore".to_owned(),
            });
        }
        if has_mbx {
            let mbx_restore = roles
                .iter()
                .position(|role| *role == Some(StepRole::MbxCache));
            if mbx_restore.is_some_and(|restore_at| fetch_at < restore_at) {
                return Err(MiseError::CacheNotEligible {
                    task: "fetch".to_owned(),
                    reason: "fetch_before_mbx".to_owned(),
                });
            }
        }
    }
    Ok(())
}

pub fn check_steps_before_fetch(names: &[String], required: &[&str]) -> Result<(), MiseError> {
    let at = |want: &str| names.iter().position(|name| name == want);
    let fetch = at("Fetch Cargo sources").or_else(|| {
        names
            .iter()
            .position(|n| n.starts_with("Fetch Cargo sources"))
    });
    if let Some(fetch_at) = fetch {
        for required_name in required {
            if let Some(required_at) = names.iter().position(|name| name == required_name)
                && fetch_at < required_at
            {
                return Err(MiseError::CacheNotEligible {
                    task: "fetch".to_owned(),
                    reason: "fetch_before_required_step".to_owned(),
                });
            }
        }
    }
    Ok(())
}

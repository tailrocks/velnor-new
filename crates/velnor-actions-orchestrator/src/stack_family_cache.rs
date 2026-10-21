//! Family cache plans and step groups of the Bun, dbt and Ansible jobs.
//!
//! Each job builds one plan per family it caches. [`prelude`] returns the
//! key step and the read-only restores; [`after_producer`] returns the size
//! admission and the trusted-push save that sit right after the family's
//! producer step, so later failing checks never starve the cache.

use velnor_actions_contract::Step;
use velnor_actions_contract::config::AnsibleCollection;
use velnor_actions_contract::workflow::cache_family::CacheFamily;
use velnor_actions_contract::workflow::cache_family_steps::root_locator;
use velnor_actions_workflow_renderer::steps::{
    FamilyCachePlan, FamilySnapshot, family_keys_step, family_restore_step, family_save_step,
    family_size_step,
};

use crate::OrchestratorError;

fn joined(dir: &str, file: &str) -> String {
    if dir.is_empty() || dir == "." {
        file.to_owned()
    } else {
        format!("{dir}/{file}")
    }
}

fn locator(root: &str) -> Result<String, OrchestratorError> {
    root_locator(root).ok_or_else(|| OrchestratorError::Contract {
        problem: format!("family_cache_root:{root}"),
    })
}

/// Bun's package cache of one workspace root (content-addressed, so a restore
/// from another lock of the same root is valid).
pub(crate) fn bun_install(root: &str) -> Result<FamilyCachePlan, OrchestratorError> {
    Ok(FamilyCachePlan {
        family: CacheFamily::BunInstall,
        locator: None,
        snapshot: FamilySnapshot::Files(vec![joined(root, "bun.lock")]),
        compat: vec![
            "bun=${VELNOR_TOOL_BUN:-}".to_owned(),
            format!("root={}", locator(root)?),
        ],
    })
}

/// The hash-pinned Playwright browser directory.
pub(crate) fn playwright_browsers() -> FamilyCachePlan {
    FamilyCachePlan {
        family: CacheFamily::PlaywrightBrowsers,
        locator: None,
        snapshot: FamilySnapshot::Files(vec![".velnor/playwright.lock".to_owned()]),
        compat: Vec::new(),
    }
}

/// One dbt project's installed packages.
pub(crate) fn dbt_packages(dir: &str) -> Result<FamilyCachePlan, OrchestratorError> {
    Ok(FamilyCachePlan {
        family: CacheFamily::DbtPackages,
        locator: Some(locator(dir)?),
        snapshot: FamilySnapshot::Files(vec![joined(dir, "package-lock.yml")]),
        compat: vec!["dbt-fusion=${VELNOR_TOOL_DBT_FUSION:-}".to_owned()],
    })
}

/// uv's wheel cache, keyed by the hash-pinned requirements file.
pub(crate) fn python_venv(requirements: &str) -> FamilyCachePlan {
    FamilyCachePlan {
        family: CacheFamily::PythonVenv,
        locator: None,
        snapshot: FamilySnapshot::Files(vec![requirements.to_owned()]),
        compat: vec![
            "uv=${VELNOR_TOOL_UV:-}".to_owned(),
            "python=${VELNOR_TOOL_PYTHON:-}".to_owned(),
        ],
    }
}

/// The verified collection tarballs, keyed by the declared set.
pub(crate) fn ansible_collections(collections: &[AnsibleCollection]) -> FamilyCachePlan {
    let mut lines: Vec<String> = collections
        .iter()
        .map(|c| format!("{} {} {}", c.name, c.version, c.sha256))
        .collect();
    lines.sort();
    FamilyCachePlan {
        family: CacheFamily::AnsibleCollections,
        locator: None,
        snapshot: FamilySnapshot::Lines(lines),
        compat: Vec::new(),
    }
}

/// The key step plus one read-only restore per plan.
///
/// # Errors
///
/// Returns render errors for an unsafe plan.
pub(crate) fn prelude(plans: &[FamilyCachePlan]) -> Result<Vec<Step>, OrchestratorError> {
    let mut steps = vec![family_keys_step(plans)?];
    for plan in plans {
        steps.push(family_restore_step(plan)?);
    }
    Ok(steps)
}

/// Size admission and trusted-push save, placed right after the producer.
///
/// # Errors
///
/// Returns render errors for an unsafe plan.
pub(crate) fn after_producer(plan: &FamilyCachePlan) -> Result<Vec<Step>, OrchestratorError> {
    Ok(vec![family_size_step(plan)?, family_save_step(plan)?])
}

#[cfg(test)]
#[path = "stack_family_cache_tests.rs"]
mod tests;

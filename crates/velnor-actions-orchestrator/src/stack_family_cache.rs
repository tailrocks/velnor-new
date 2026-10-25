use velnor_actions_contract::Step;
use velnor_actions_contract::config::AnsibleCollection;
use velnor_actions_contract::workflow::cache_family::CacheFamily;
use velnor_actions_contract::workflow::cache_family_steps::root_locator;
use velnor_actions_workflow_renderer::steps::{
    FamilyCachePlan, FamilySnapshot, family_keys_step, family_keys_step_single_writer,
    family_restore_step, family_save_step, family_size_step,
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

pub(crate) fn playwright_browsers() -> FamilyCachePlan {
    FamilyCachePlan {
        family: CacheFamily::PlaywrightBrowsers,
        locator: None,
        snapshot: FamilySnapshot::Files(vec![".velnor/playwright.lock".to_owned()]),
        compat: Vec::new(),
    }
}

pub(crate) fn dbt_packages(dir: &str) -> Result<FamilyCachePlan, OrchestratorError> {
    Ok(FamilyCachePlan {
        family: CacheFamily::DbtPackages,
        locator: Some(locator(dir)?),
        snapshot: FamilySnapshot::Files(vec![joined(dir, "package-lock.yml")]),
        compat: vec!["dbt-fusion=${VELNOR_TOOL_DBT_FUSION:-}".to_owned()],
    })
}

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

pub(crate) fn prelude(plans: &[FamilyCachePlan]) -> Result<Vec<Step>, OrchestratorError> {
    prelude_for(plans, false)
}

pub(crate) fn prelude_for(
    plans: &[FamilyCachePlan],
    single_writer: bool,
) -> Result<Vec<Step>, OrchestratorError> {
    let keys = if single_writer {
        family_keys_step_single_writer(plans)?
    } else {
        family_keys_step(plans)?
    };
    let mut steps = vec![keys];
    for plan in plans {
        steps.push(family_restore_step(plan)?);
    }
    Ok(steps)
}

pub(crate) fn after_producer(plan: &FamilyCachePlan) -> Result<Vec<Step>, OrchestratorError> {
    Ok(vec![family_size_step(plan)?, family_save_step(plan)?])
}

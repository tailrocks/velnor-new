use std::collections::BTreeSet;
use std::path::Path;

use velnor_actions_contract::config::DbtStackConfig;
use velnor_actions_contract::{FileIndex, VelnorConfig};
use velnor_actions_dbt::{
    FusionVersion, PACKAGE_LOCK_FILE, PACKAGES_FILE, PROFILES_FILE, PROJECT_FILE, Project,
    ProjectFiles, analyze_project, check_mise_lock, job_id, parse_fusion_version,
};

use super::stack_support::{declared_version, read_lock, read_text};
use crate::OrchestratorError;
use crate::config::CONFIG_REL;

const FUSION_KEYS: [&str; 2] = ["dbt-fusion", "aqua:getdbt.com/dbt-fusion"];

#[derive(Debug, Clone, Default)]
pub(crate) struct DbtInventory {
    pub(crate) projects: Vec<Project>,
}

impl DbtInventory {
    pub(crate) fn dirs(&self) -> Vec<String> {
        self.projects
            .iter()
            .map(|project| project.dir.clone())
            .collect()
    }

    pub(crate) fn for_config(
        root: &Path,
        index: &FileIndex,
        config: &VelnorConfig,
    ) -> Result<Self, OrchestratorError> {
        match &config.stacks.dbt {
            Some(stack) => Self::load(root, index, stack),
            None => Ok(Self::default()),
        }
    }

    pub(crate) fn load(
        root: &Path,
        index: &FileIndex,
        config: &DbtStackConfig,
    ) -> Result<Self, OrchestratorError> {
        let mut findings = Vec::new();
        let mut projects = Vec::new();
        let mut ids = BTreeSet::new();
        for configured in &config.projects {
            let dir = configured.as_str();
            let (project, found) = analyze(root, dir)?;
            findings.extend(found.into_iter().map(|code| format!("{dir}: {code}")));
            if let Some(project) = project {
                if !ids.insert(job_id(&project)) {
                    return Err(config_error(
                        "stacks.dbt.projects",
                        &format!("dbt_job_id_duplicate:{dir}"),
                    ));
                }
                projects.push(project);
            }
        }
        let version = fusion_version(root, index)?;
        findings.extend(lock_findings(root, version)?);
        if findings.is_empty() {
            Ok(Self { projects })
        } else {
            Err(OrchestratorError::Contract {
                problem: format!("dbt_stack_findings:{}", findings.join("; ")),
            })
        }
    }
}

fn join(dir: &str, name: &str) -> String {
    if dir == "." {
        name.to_owned()
    } else {
        format!("{dir}/{name}")
    }
}

fn analyze(root: &Path, dir: &str) -> Result<(Option<Project>, Vec<String>), OrchestratorError> {
    let Some(project) = read_text(root, &join(dir, PROJECT_FILE))? else {
        return Ok((None, vec!["dbt_project_missing".to_owned()]));
    };
    let packages = read_text(root, &join(dir, PACKAGES_FILE))?;
    let lock = read_text(root, &join(dir, PACKAGE_LOCK_FILE))?;
    let profiles = read_text(root, &join(dir, PROFILES_FILE))?;
    let files = ProjectFiles {
        project: &project,
        packages: packages.as_deref(),
        lock: lock.as_deref(),
        profiles: profiles.as_deref(),
    };
    let (model, found) = analyze_project(dir, &files);
    Ok((
        model,
        found.iter().map(|error| error.code().to_owned()).collect(),
    ))
}

fn fusion_version(root: &Path, index: &FileIndex) -> Result<FusionVersion, OrchestratorError> {
    let text = declared_version(root, index, &FUSION_KEYS, "stacks.dbt")?
        .ok_or_else(|| config_error("stacks.dbt", "dbt_fusion_not_declared"))?;
    parse_fusion_version(&text).map_err(|error| config_error("stacks.dbt", error.code()))
}

fn lock_findings(root: &Path, version: FusionVersion) -> Result<Vec<String>, OrchestratorError> {
    Ok(match read_lock(root)? {
        Some(text) => check_mise_lock(&text, version)
            .iter()
            .map(|error| error.code().to_owned())
            .collect(),
        None => vec!["dbt_fusion_lock_missing".to_owned()],
    })
}

fn config_error(key: &str, problem: &str) -> OrchestratorError {
    OrchestratorError::config(CONFIG_REL, key, problem)
}

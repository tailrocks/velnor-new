//! Read the dbt projects `[stacks.dbt]` names from the checked-out repository.
//!
//! One pass over the file index yields every analysed project and the exact
//! `dbt-fusion` version the project's Mise config declares. Generation (jobs)
//! and planning (selection) both start here. Any adapter finding fails
//! generation with its machine-readable code: a job is never emitted for a
//! project that violates the stack contract.

use std::collections::BTreeSet;
use std::path::Path;

use velnor_actions_contract::config::DbtStackConfig;
use velnor_actions_contract::config::project_tool_refs::{
    PROJECT_MISE_CONFIG_PATTERNS, PROJECT_TOOL_LOCK_FILES, matches_source_pattern,
};
use velnor_actions_contract::{FileIndex, VelnorConfig};
use velnor_actions_dbt::{
    FusionVersion, PACKAGE_LOCK_FILE, PACKAGES_FILE, PROFILES_FILE, PROJECT_FILE, Project,
    ProjectFiles, analyze_project, check_mise_lock, job_id, parse_fusion_version,
};

use crate::OrchestratorError;
use crate::config::CONFIG_REL;
use crate::safe_read::{MAX_REPO_FILE_BYTES, RepoRead, read_repo_file};

/// `[tools]` keys that declare dbt-fusion (the resolver reads the same set).
const FUSION_KEYS: [&str; 2] = ["dbt-fusion", "aqua:getdbt.com/dbt-fusion"];

/// Every configured dbt project plus the declared Fusion version.
#[derive(Debug, Clone, Default)]
pub(crate) struct DbtInventory {
    /// Projects in configuration order.
    pub(crate) projects: Vec<Project>,
}

impl DbtInventory {
    /// Repository-relative project directories, in configuration order.
    pub(crate) fn dirs(&self) -> Vec<String> {
        self.projects
            .iter()
            .map(|project| project.dir.clone())
            .collect()
    }

    /// The inventory of the configured `[stacks.dbt]` (empty without it).
    ///
    /// # Errors
    ///
    /// The errors of [`Self::load`].
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

    /// Load and cross-check everything `config` names.
    ///
    /// # Errors
    ///
    /// Fails closed with `dbt_stack_findings:` followed by every adapter code,
    /// or with a typed configuration error.
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

fn read(root: &Path, rel: &str) -> Result<Option<String>, OrchestratorError> {
    match read_repo_file(root, rel, MAX_REPO_FILE_BYTES)? {
        RepoRead::Text(text) => Ok(Some(text)),
        RepoRead::Absent => Ok(None),
    }
}

fn analyze(root: &Path, dir: &str) -> Result<(Option<Project>, Vec<String>), OrchestratorError> {
    let Some(project) = read(root, &join(dir, PROJECT_FILE))? else {
        return Ok((None, vec!["dbt_project_missing".to_owned()]));
    };
    let packages = read(root, &join(dir, PACKAGES_FILE))?;
    let lock = read(root, &join(dir, PACKAGE_LOCK_FILE))?;
    let profiles = read(root, &join(dir, PROFILES_FILE))?;
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

/// The one Fusion version every Mise config declares, or an error.
fn fusion_version(root: &Path, index: &FileIndex) -> Result<FusionVersion, OrchestratorError> {
    let mut declared: Option<(String, String)> = None;
    for file in index.files().iter().filter(|file| {
        PROJECT_MISE_CONFIG_PATTERNS
            .iter()
            .any(|pattern| matches_source_pattern(pattern, file))
    }) {
        let Some(text) = read(root, file)? else {
            continue;
        };
        let value: toml::Value = toml::from_str(&text)
            .map_err(|_| config_error("stacks.dbt", &format!("mise_config_not_toml:{file}")))?;
        let Some(tools) = value.get("tools").and_then(toml::Value::as_table) else {
            continue;
        };
        for key in FUSION_KEYS {
            let Some(entry) = tools.get(key) else {
                continue;
            };
            let version = match entry {
                toml::Value::String(text) => Some(text.clone()),
                toml::Value::Table(table) => table
                    .get("version")
                    .and_then(toml::Value::as_str)
                    .map(str::to_owned),
                _ => None,
            };
            let Some(version) = version else { continue };
            match &declared {
                Some((_, first)) if *first != version => {
                    return Err(config_error(
                        "stacks.dbt",
                        &format!("conflicting_project_tool_versions:dbt-fusion,{file}"),
                    ));
                }
                Some(_) => {}
                None => declared = Some((file.clone(), version)),
            }
        }
    }
    let (_, text) =
        declared.ok_or_else(|| config_error("stacks.dbt", "dbt_fusion_not_declared"))?;
    parse_fusion_version(&text).map_err(|error| config_error("stacks.dbt", error.code()))
}

/// Findings of the repository `mise.lock` for the declared Fusion version.
fn lock_findings(root: &Path, version: FusionVersion) -> Result<Vec<String>, OrchestratorError> {
    for name in PROJECT_TOOL_LOCK_FILES {
        if let Some(text) = read(root, name)? {
            return Ok(check_mise_lock(&text, version)
                .iter()
                .map(|error| error.code().to_owned())
                .collect());
        }
    }
    Ok(vec!["dbt_fusion_lock_missing".to_owned()])
}

fn config_error(key: &str, problem: &str) -> OrchestratorError {
    OrchestratorError::config(CONFIG_REL, key, problem)
}

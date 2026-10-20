//! Read the Ansible roots `[stacks.ansible]` names from the checked-out repository.
//!
//! One pass over the file index yields every analysed root: its sorted
//! top-level playbooks and the digest of its `ansible.cfg`. Every adapter
//! finding fails generation with its machine-readable code, so a job is never
//! emitted for a root that violates the stack contract (vault files, an
//! unhashed or disagreeing Python lock, unpinned collections, unsafe
//! `ansible.cfg` keys, a `mise.lock` without checksums for uv and Python).

use std::path::Path;

use velnor_actions_ansible::locked::{PYTHON, UV};
use velnor_actions_ansible::{
    CORE_DISTRIBUTION, LINT_DISTRIBUTION, check_agreement, check_ansible_cfg, check_locked_tool,
    check_vault, parse_requirements, parse_requirements_yaml, top_level_playbooks,
    verify_collections,
};
use velnor_actions_contract::config::{AnsibleCheck, AnsibleStackConfig};
use velnor_actions_contract::{FileIndex, VelnorConfig, slugify_segment};

use super::stack_support::{declared_version, read_lock, read_text};
use crate::OrchestratorError;
use crate::config::CONFIG_REL;
use crate::generator::sha256_hex;
use crate::safe_read::{MAX_REPO_FILE_BYTES, RepoBytes, read_repo_bytes};

/// `ansible.cfg` of a root.
pub(crate) const CFG_NAME: &str = "ansible.cfg";
/// Collection pins of a root.
pub(crate) const REQUIREMENTS_NAME: &str = "requirements.yaml";

/// One analysed root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AnsibleRoot {
    /// Repository-relative directory (`.` for the repository root).
    pub(crate) dir: String,
    /// Sorted top-level playbooks, repository-relative.
    pub(crate) playbooks: Vec<String>,
    /// SHA-256 of the root's `ansible.cfg`.
    pub(crate) cfg_sha256: String,
}

impl AnsibleRoot {
    /// Generated job id (`ansible-<slug>`).
    pub(crate) fn job_id(&self) -> String {
        format!(
            "{}{}",
            velnor_actions_contract::config::ANSIBLE_JOB_PREFIX,
            slugify_segment(&self.dir)
        )
    }

    /// Display name (`Ansible / <dir>`).
    pub(crate) fn job_name(&self) -> String {
        format!("Ansible / {}", self.dir)
    }
}

/// Every configured Ansible root.
#[derive(Debug, Clone, Default)]
pub(crate) struct AnsibleInventory {
    /// Roots in configuration order.
    pub(crate) roots: Vec<AnsibleRoot>,
    /// Files every root job reads: the Python requirements and the inventory.
    pub(crate) shared: Vec<String>,
}

fn join(dir: &str, name: &str) -> String {
    if dir == "." {
        name.to_owned()
    } else {
        format!("{dir}/{name}")
    }
}

impl AnsibleInventory {
    /// The inventory of the configured `[stacks.ansible]` (empty without it).
    ///
    /// # Errors
    ///
    /// Fails closed with `ansible_stack_findings:` followed by every code.
    pub(crate) fn for_config(
        root: &Path,
        index: &FileIndex,
        config: &VelnorConfig,
    ) -> Result<Self, OrchestratorError> {
        match &config.stacks.ansible {
            Some(stack) => Self::load(root, index, stack),
            None => Ok(Self::default()),
        }
    }

    fn load(
        root: &Path,
        index: &FileIndex,
        config: &AnsibleStackConfig,
    ) -> Result<Self, OrchestratorError> {
        let mut findings = tool_findings(root, index, config)?;
        let mut roots = Vec::new();
        for configured in &config.roots {
            let dir = configured.as_str();
            let (found, analysed) = analyse_root(root, index, config, dir);
            findings.extend(found.into_iter().map(|code| format!("{dir}: {code}")));
            roots.extend(analysed);
        }
        let mut ids = std::collections::BTreeSet::new();
        for analysed in &roots {
            if !ids.insert(analysed.job_id()) {
                return Err(config_error(&format!(
                    "ansible_job_id_duplicate:{}",
                    analysed.dir
                )));
            }
        }
        if !index.files().contains(&config.inventory) {
            findings.push("ansible_inventory_missing".to_owned());
        }
        if findings.is_empty() {
            Ok(Self {
                roots,
                shared: vec![config.python_requirements.clone(), config.inventory.clone()],
            })
        } else {
            Err(OrchestratorError::Contract {
                problem: format!("ansible_stack_findings:{}", findings.join("; ")),
            })
        }
    }
}

/// Python lock, Mise declarations and `mise.lock` rows shared by every root.
fn tool_findings(
    root: &Path,
    index: &FileIndex,
    config: &AnsibleStackConfig,
) -> Result<Vec<String>, OrchestratorError> {
    let mut findings = Vec::new();
    let declared = |keys: &[&str]| declared_version(root, index, keys, "stacks.ansible");
    for (keys, code) in [
        (&["python", "core:python"][..], "python_not_declared"),
        (&["uv", "aqua:astral-sh/uv"][..], "uv_not_declared"),
    ] {
        if declared(keys)?.is_none() {
            findings.push(code.to_owned());
        }
    }
    let lock = read_lock(root)?;
    for tool in [UV, PYTHON] {
        match &lock {
            Some(text) => findings.extend(
                check_locked_tool(text, tool)
                    .iter()
                    .map(|error| error.code().to_owned()),
            ),
            None => findings.push(format!("ansible_tool_lock_missing:{}", tool.key)),
        }
    }
    let Some(text) = read_text(root, &config.python_requirements)? else {
        findings.push("python_requirements_missing".to_owned());
        return Ok(findings);
    };
    let requirements = match parse_requirements(&text) {
        Ok(requirements) => requirements,
        Err(errors) => {
            findings.extend(errors.iter().map(|error| error.code().to_owned()));
            return Ok(findings);
        }
    };
    let mut agree = vec![(
        CORE_DISTRIBUTION,
        &["ansible-core", "pypi:ansible-core"][..],
        "ansible_core_not_declared",
    )];
    if config.effective_checks().contains(&AnsibleCheck::Lint) {
        agree.push((
            LINT_DISTRIBUTION,
            &["ansible-lint", "pypi:ansible-lint", "pipx:ansible-lint"][..],
            "ansible_lint_not_declared",
        ));
    }
    for (distribution, keys, missing) in agree {
        match declared(keys)? {
            None => findings.push(missing.to_owned()),
            Some(version) => {
                if let Err(error) = check_agreement(&requirements, distribution, &version) {
                    findings.push(error.code().to_owned());
                }
            }
        }
    }
    Ok(findings)
}

/// Text files below `dir`, `(path, text)`; non-UTF-8 and oversize files are skipped.
fn root_files(root: &Path, index: &FileIndex, dir: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for file in index
        .files()
        .iter()
        .filter(|file| velnor_actions_ansible::paths::is_under(file, dir))
    {
        if let Ok(RepoBytes::Bytes(bytes)) = read_repo_bytes(root, file, MAX_REPO_FILE_BYTES)
            && let Ok(text) = String::from_utf8(bytes)
        {
            out.push((file.clone(), text));
        }
    }
    out
}

fn analyse_root(
    root: &Path,
    index: &FileIndex,
    config: &AnsibleStackConfig,
    dir: &str,
) -> (Vec<String>, Option<AnsibleRoot>) {
    let files = root_files(root, index, dir);
    let pairs: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    let mut findings: Vec<String> = check_vault(&pairs)
        .iter()
        .map(|error| error.code().to_owned())
        .collect();
    let cfg_path = join(dir, CFG_NAME);
    let cfg = pairs.iter().find(|(path, _)| *path == cfg_path);
    match cfg {
        Some((_, text)) => findings.extend(
            check_ansible_cfg(text)
                .iter()
                .map(|error| error.code().to_owned()),
        ),
        None => findings.push("ansible_cfg_missing".to_owned()),
    }
    let requirements = pairs
        .iter()
        .find(|(path, _)| *path == join(dir, REQUIREMENTS_NAME));
    match requirements.map(|(_, text)| parse_requirements_yaml(text)) {
        Some(Err(error)) => findings.push(error.code().to_owned()),
        Some(Ok(pins)) => findings.extend(
            verify_collections(&config.collections, &pins)
                .iter()
                .map(|error| error.code().to_owned()),
        ),
        None => findings.extend(
            verify_collections(&config.collections, &std::collections::BTreeMap::new())
                .iter()
                .map(|error| error.code().to_owned()),
        ),
    }
    let playbooks = top_level_playbooks(dir, &pairs);
    if config.effective_checks().contains(&AnsibleCheck::Syntax) && playbooks.is_empty() {
        findings.push("ansible_no_playbooks".to_owned());
    }
    let root = cfg.map(|(_, text)| AnsibleRoot {
        dir: dir.to_owned(),
        playbooks,
        cfg_sha256: sha256_hex(text.as_bytes()),
    });
    (findings, root)
}

fn config_error(problem: &str) -> OrchestratorError {
    OrchestratorError::config(CONFIG_REL, "stacks.ansible", problem)
}

//! Read the Bun workspaces `[stacks.bun]` names from the checked-out repository.
//!
//! One pass over the file index and the lock/manifest texts yields every
//! workspace, its members and the static outside-read evidence. Generation
//! (jobs) and planning (selection) both start here, so they cannot disagree
//! about what a member is.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use velnor_actions_bun::paths::{is_under, under_root};
use velnor_actions_bun::{
    BunError, Workspace, check_js_lockfiles, is_config_file, outside_reads, parse_lock, resolve,
    slug,
};
use velnor_actions_contract::FileIndex;
use velnor_actions_contract::config::BunStackConfig;

use crate::OrchestratorError;
use crate::config::CONFIG_REL;
use crate::safe_read::{MAX_REPO_FILE_BYTES, RepoRead, read_repo_file};

/// One configured root: its resolved workspace and repository `bunfig.toml`.
#[derive(Debug, Clone)]
pub(crate) struct BunRoot {
    /// Members, sorted by name.
    pub(crate) workspace: Workspace,
    /// Repository `bunfig.toml` text, when present.
    pub(crate) bunfig: Option<String>,
}

/// Every configured Bun workspace plus per-member outside reads.
#[derive(Debug, Clone, Default)]
pub(crate) struct BunInventory {
    /// Roots in configuration order.
    pub(crate) roots: Vec<BunRoot>,
    /// Outside reads per member name (empty vectors are omitted).
    pub(crate) reads: BTreeMap<String, Vec<String>>,
}

impl BunInventory {
    /// Every workspace, in configuration order.
    pub(crate) fn workspaces(&self) -> Vec<Workspace> {
        self.roots
            .iter()
            .map(|root| root.workspace.clone())
            .collect()
    }

    /// Load and cross-check everything `config` names.
    ///
    /// # Errors
    ///
    /// Fails closed with the adapter's machine-readable code, prefixed by
    /// `stacks.bun`, for any lockfile, manifest or configuration problem.
    pub(crate) fn load(
        root: &Path,
        index: &FileIndex,
        config: &BunStackConfig,
    ) -> Result<Self, OrchestratorError> {
        let mut inventory = Self::default();
        for configured in &config.roots {
            let prefix = configured.unit_prefix();
            let loaded = load_root(root, index, prefix).map_err(|error| adapter_error(&error))?;
            inventory
                .reads
                .extend(root_reads(root, index, &loaded.workspace)?);
            inventory.roots.push(loaded);
        }
        inventory.check_names(config)?;
        Ok(inventory)
    }

    fn check_names(&self, config: &BunStackConfig) -> Result<(), OrchestratorError> {
        let mut names = BTreeSet::new();
        let mut slugs = BTreeSet::new();
        for member in self.roots.iter().flat_map(|root| &root.workspace.members) {
            if !names.insert(member.name.as_str()) || !slugs.insert(slug(&member.name)) {
                return Err(config_error(
                    "stacks.bun.roots",
                    &format!("bun_member_name_duplicate:{}", member.name),
                ));
            }
        }
        let referenced = config
            .members
            .keys()
            .chain(config.inputs.iter().flat_map(|rule| &rule.members));
        for name in referenced {
            if !names.contains(name.as_str()) {
                return Err(config_error(
                    "stacks.bun.members",
                    &format!("unknown_bun_member:{name}"),
                ));
            }
        }
        Ok(())
    }
}

fn load_root(root: &Path, index: &FileIndex, prefix: &str) -> Result<BunRoot, BunError> {
    let direct: Vec<&str> = index
        .files()
        .iter()
        .filter_map(|file| direct_child(file, prefix))
        .collect();
    check_js_lockfiles(&direct)?;
    let package = required(root, &under_root(prefix, "package.json"))?;
    let lock = required(root, &under_root(prefix, "bun.lock"))?;
    let mut members = BTreeMap::new();
    for key in parse_lock(&lock)?
        .workspaces
        .keys()
        .filter(|key| !key.is_empty())
    {
        members.insert(
            key.clone(),
            required(root, &under_root(&under_root(prefix, key), "package.json"))?,
        );
    }
    let workspace = resolve(prefix, &package, &lock, &members)?;
    let bunfig = read(root, &under_root(prefix, "bunfig.toml"))?;
    Ok(BunRoot { workspace, bunfig })
}

/// `file` relative to `prefix` when it is a direct child (no further `/`).
fn direct_child<'a>(file: &'a str, prefix: &str) -> Option<&'a str> {
    let rel = if prefix.is_empty() {
        file
    } else {
        file.strip_prefix(prefix)?.strip_prefix('/')?
    };
    (!rel.contains('/')).then_some(rel)
}

fn read(root: &Path, rel: &str) -> Result<Option<String>, BunError> {
    match read_repo_file(root, rel, MAX_REPO_FILE_BYTES) {
        Ok(RepoRead::Text(text)) => Ok(Some(text)),
        Ok(RepoRead::Absent) => Ok(None),
        Err(_) => Err(BunError::new("bun_file_unreadable", rel)),
    }
}

fn required(root: &Path, rel: &str) -> Result<String, BunError> {
    read(root, rel)?.ok_or_else(|| BunError::new("bun_file_missing", rel))
}

/// Config files of each member whose reads can leave its directory.
fn root_reads(
    root: &Path,
    index: &FileIndex,
    workspace: &Workspace,
) -> Result<BTreeMap<String, Vec<String>>, OrchestratorError> {
    let mut out = BTreeMap::new();
    for member in &workspace.members {
        let mut files = Vec::new();
        for file in index
            .files()
            .iter()
            .filter(|file| is_under(file, &member.dir))
        {
            let name = file.rsplit('/').next().unwrap_or(file);
            if is_config_file(name)
                && !file.split('/').any(|part| part == "node_modules")
                && let Some(text) = read(root, file).map_err(|error| adapter_error(&error))?
            {
                files.push((file.clone(), text));
            }
        }
        let reads = outside_reads(&member.dir, &files);
        if !reads.is_empty() {
            out.insert(member.name.clone(), reads);
        }
    }
    Ok(out)
}

fn adapter_error(error: &BunError) -> OrchestratorError {
    config_error("stacks.bun", error.code())
}

fn config_error(key: &str, problem: &str) -> OrchestratorError {
    OrchestratorError::config(CONFIG_REL, key, problem)
}

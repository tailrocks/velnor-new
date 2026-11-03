// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::Path;

use velnor_actions_contract::config::project_tool_refs::{
    is_declaration_source, is_project_tool_lock,
};
use velnor_actions_contract::{
    NON_INPUTS_FIELD, VelnorConfig, glob_may_overlap, glob_may_reach_tree, matches_glob,
};
use velnor_actions_mise::GitRequest;
use velnor_actions_rust::{SelectionBroadening, is_known_toolfile, selection_broadening};

use crate::OrchestratorError;
use crate::config::CONFIG_REL;
use crate::discover::Discovery;
use crate::git_paths::split_nul_paths;
use crate::select_affected::Ownership;
use crate::select_reads::read_target_paths;
use crate::select_symlinks::tracked_links;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Overlap {
    Github,
    Velnor,
    PackageDir,
    DeclaredInput,
    ReadTarget,
    Symlink,
    Lockfile,
    ToolFile,
}

impl Overlap {
    const fn token(self) -> &'static str {
        match self {
            Self::Github => "github",
            Self::Velnor => "velnor",
            Self::PackageDir => "package_dir",
            Self::DeclaredInput => "declared_input",
            Self::ReadTarget => "read_target",
            Self::Symlink => "symlink",
            Self::Lockfile => "lockfile",
            Self::ToolFile => "tool_file",
        }
    }
}

struct Claims<'a> {
    tracked: BTreeSet<String>,
    ownership: Ownership<'a>,
    declared: Vec<String>,
    read_targets: BTreeSet<String>,
    linked: Vec<LinkedPath>,
}

struct LinkedPath {
    path: String,
    tree: bool,
}

pub(crate) fn validate(
    root: &Path,
    config: &VelnorConfig,
    discovery: &Discovery,
) -> Result<(), OrchestratorError> {
    let non_inputs = &discovery.affected.non_inputs;
    if non_inputs.is_empty() {
        return Ok(());
    }
    let claims = Claims::collect(root, config, discovery).map_err(|problem| {
        OrchestratorError::config(
            CONFIG_REL,
            NON_INPUTS_FIELD,
            format!("non_input_scan_failed:{problem}"),
        )
    })?;
    for glob in non_inputs.globs() {
        if let Some(kind) = claims.overlap(glob) {
            return Err(OrchestratorError::config(
                CONFIG_REL,
                NON_INPUTS_FIELD,
                format!("non_input_overlaps_claim:{}:{glob}", kind.token()),
            ));
        }
    }
    Ok(())
}

impl<'a> Claims<'a> {
    fn collect(
        root: &Path,
        config: &VelnorConfig,
        discovery: &'a Discovery,
    ) -> Result<Self, String> {
        let output = GitRequest::ls_files(vec![OsString::from("-z")])
            .run_in(root)
            .map_err(|err| err.to_string())?;
        output
            .require_success("git")
            .map_err(|err| err.to_string())?;
        let tracked: BTreeSet<String> = split_nul_paths(&output.stdout)?;
        let ownership = Ownership::new(discovery);
        let mut declared: Vec<String> = discovery
            .affected
            .rules
            .iter()
            .flat_map(|rule| rule.globs.iter().cloned())
            .collect();
        declared.extend(config.checks.iter().flat_map(|check| check.inputs.clone()));
        let read_targets = read_target_paths(root, &ownership)?;
        let mut linked = Vec::new();
        for outcome in tracked_links(root)? {
            match outcome {
                Ok(link) => {
                    linked.push(LinkedPath {
                        path: link.link,
                        tree: false,
                    });
                    linked.extend(link.chain.into_iter().map(|path| LinkedPath {
                        path,
                        tree: false,
                    }));
                    linked.push(LinkedPath {
                        path: link.target,
                        tree: true,
                    });
                }
                Err((path, _)) => linked.push(LinkedPath { path, tree: false }),
            }
        }
        Ok(Self {
            tracked,
            ownership,
            declared,
            read_targets,
            linked,
        })
    }

    fn overlap(&self, glob: &str) -> Option<Overlap> {
        let tracked_hit = |claim: &dyn Fn(&str) -> bool| {
            self.tracked
                .iter()
                .any(|file| matches_glob(glob, file) && claim(file))
        };
        let reaches = |path: &str, tree: bool| {
            matches_glob(glob, path)
                || (tree
                    && tracked_hit(&|file| {
                        path.is_empty() || file.strip_prefix(path).is_some_and(|rest| rest.starts_with('/'))
                    }))
        };
        if glob_may_reach_tree(glob, ".github") {
            Some(Overlap::Github)
        } else if glob_may_reach_tree(glob, ".velnor") {
            Some(Overlap::Velnor)
        } else if self
            .ownership
            .owner_dirs()
            .any(|dir| glob_may_reach_tree(glob, dir))
        {
            Some(Overlap::PackageDir)
        } else if self
            .declared
            .iter()
            .any(|declared| glob_may_overlap(glob, declared))
        {
            Some(Overlap::DeclaredInput)
        } else if self.read_targets.iter().any(|target| reaches(target, true)) {
            Some(Overlap::ReadTarget)
        } else if self.linked.iter().any(|link| reaches(&link.path, link.tree)) {
            Some(Overlap::Symlink)
        } else if tracked_hit(&|file| {
            selection_broadening(file) == Some(SelectionBroadening::Lockfile)
                || is_project_tool_lock(file)
        }) {
            Some(Overlap::Lockfile)
        } else if tracked_hit(&|file| {
            is_known_toolfile(file)
                || selection_broadening(file) == Some(SelectionBroadening::RootConfig)
                || is_declaration_source(file)
                || self.ownership.is_tool_config(file)
        }) {
            Some(Overlap::ToolFile)
        } else {
            None
        }
    }
}

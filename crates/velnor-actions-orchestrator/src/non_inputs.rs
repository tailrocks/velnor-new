// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::Path;

use velnor_actions_contract::config::project_tool_refs::{
    is_declaration_source, is_project_tool_lock,
};
use velnor_actions_contract::{
    NON_INPUTS_FIELD, NonInputs, VelnorConfig, glob_may_overlap, non_input_covers,
    non_input_may_overlap, non_input_may_reach_tree,
};
use velnor_actions_mise::GitRequest;
use velnor_actions_rust::{SelectionBroadening, is_known_toolfile, selection_broadening};

use crate::OrchestratorError;
use crate::config::CONFIG_REL;
use crate::discover::Discovery;
use crate::git_paths::split_nul_paths;
use crate::select_affected::Ownership;
use crate::select_reads::{UnverifiedKind, read_summary};
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
    rule_globs: Vec<String>,
    declared: Vec<String>,
    read_targets: BTreeSet<String>,
    unverified: BTreeSet<(String, UnverifiedKind)>,
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
    if claims.ownership.owner_dirs().any(str::is_empty) {
        return Err(unsupported("root_package"));
    }
    for glob in non_inputs.globs() {
        if let Some(kind) = claims.overlap(glob) {
            return Err(OrchestratorError::config(
                CONFIG_REL,
                NON_INPUTS_FIELD,
                format!("non_input_overlaps_claim:{}:{glob}", kind.token()),
            ));
        }
    }
    if let Some((package, kind)) = claims.unverified.first() {
        return Err(unsupported(&format!("{}:{package}", kind.token())));
    }
    if let Some(check) = config
        .checks
        .iter()
        .find(|check| !claims.is_rust_input(&check.inputs))
    {
        return Err(unsupported(&format!("non_rust_check:{}", check.id)));
    }
    if let Some(path) = claims.case_conflation(non_inputs) {
        return Err(unsupported(&format!("case_conflation:{path}")));
    }
    Ok(())
}

fn unsupported(reason: &str) -> OrchestratorError {
    OrchestratorError::config(
        CONFIG_REL,
        NON_INPUTS_FIELD,
        format!("non_inputs_unsupported:{reason}"),
    )
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
        let rule_globs: Vec<String> = discovery
            .affected
            .rules
            .iter()
            .flat_map(|rule| rule.globs.iter().cloned())
            .collect();
        let mut declared = rule_globs.clone();
        declared.extend(config.checks.iter().flat_map(|check| check.inputs.clone()));
        let reads = read_summary(root, &ownership)?;
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
            rule_globs,
            declared,
            read_targets: reads.targets,
            unverified: reads.unverified,
            linked,
        })
    }

    fn overlap(&self, glob: &str) -> Option<Overlap> {
        let tracked_hit = |claim: &dyn Fn(&str) -> bool| {
            self.tracked
                .iter()
                .any(|file| non_input_covers(glob, file) && claim(file))
        };
        let reaches = |path: &str, tree: bool| {
            non_input_covers(glob, path)
                || (tree
                    && tracked_hit(&|file| {
                        path.is_empty()
                            || file
                                .strip_prefix(path)
                                .is_some_and(|rest| rest.starts_with('/'))
                    }))
        };
        if non_input_may_reach_tree(glob, ".github") {
            Some(Overlap::Github)
        } else if non_input_may_reach_tree(glob, ".velnor") {
            Some(Overlap::Velnor)
        } else if self
            .ownership
            .owner_dirs()
            .any(|dir| non_input_may_reach_tree(glob, dir))
        {
            Some(Overlap::PackageDir)
        } else if self
            .declared
            .iter()
            .any(|declared| non_input_may_overlap(glob, declared))
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

    fn is_rust_input(&self, inputs: &[String]) -> bool {
        inputs.iter().any(|input| {
            self.ownership
                .owner_dirs()
                .any(|dir| glob_may_overlap(input, &format!("{dir}/**")))
                || self
                    .rule_globs
                    .iter()
                    .any(|rule| glob_may_overlap(input, rule))
        })
    }

    fn case_conflation(&self, non_inputs: &NonInputs) -> Option<&str> {
        self.tracked
            .iter()
            .find(|path| non_inputs.contains(path) != non_inputs.contains_ignoring_case(path))
            .map(String::as_str)
    }
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::Path;

use velnor_actions_mise::GitRequest;

use crate::git_paths::split_nul_paths;
use crate::validators::{validate_diff_rev, validate_select_diff_args};

pub(super) fn tree_diff_names(root: &Path, cached: bool) -> Result<BTreeSet<String>, String> {
    let mut args = vec![OsString::from("--name-only")];
    if cached {
        args.push(OsString::from("--cached"));
    }
    args.push(OsString::from("--no-renames"));
    args.push(OsString::from("--"));
    validate_select_diff_args(&args).map_err(|err| err.to_string())?;
    args.insert(0, OsString::from("-z"));
    let output = GitRequest::diff(args)
        .run_in(root)
        .map_err(|err| err.to_string())?;
    output
        .require_success("git")
        .map_err(|err| err.to_string())?;
    split_nul_paths(&output.stdout)
}

pub(super) fn head_sha(root: &Path) -> Result<String, String> {
    let output = GitRequest::rev_parse(vec![OsString::from("HEAD")])
        .run_in(root)
        .map_err(|err| err.to_string())?;
    if !output.success {
        return Err("missing_head".to_owned());
    }
    let sha = output
        .stdout_text("git")
        .map_err(|err| err.to_string())?
        .trim()
        .to_owned();
    validate_diff_rev(&sha, "bad_head")?;
    Ok(sha)
}

pub(super) fn untracked_files(root: &Path) -> Result<BTreeSet<String>, String> {
    let args = ["--others", "--exclude-standard", "-z"]
        .iter()
        .map(OsString::from)
        .collect();
    let output = GitRequest::ls_files(args)
        .run_in(root)
        .map_err(|err| err.to_string())?;
    output
        .require_success("git")
        .map_err(|err| err.to_string())?;
    split_nul_paths(&output.stdout)
}

pub(super) fn is_advisory_toolfile(path: &str) -> bool {
    path == ".mise.toml" || velnor_actions_rust::is_known_toolfile(path)
}

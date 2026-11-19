// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use super::Event;

const NON_UTF8: &str = "non_utf8_path";
const SHORT_SHA_MIN: usize = 7;
const FULL_SHA: usize = 64;
const PROJECT_TOOL_FILES: [&str; 3] = [".tool-versions", "rust-toolchain.toml", "rust-toolchain"];
const MISE_LOCKS: [&str; 2] = ["mise.lock", ".mise.lock"];
const MISE_CONFIGS: [&str; 13] = [
    "mise.toml",
    ".mise.toml",
    "mise.*.toml",
    ".mise.*.toml",
    ".config/mise.toml",
    ".config/mise.*.toml",
    ".config/mise/config.toml",
    ".config/mise/config.*.toml",
    ".config/mise/conf.d/*.toml",
    "mise/config.toml",
    "mise/config.*.toml",
    ".mise/config.toml",
    ".mise/config.*.toml",
];

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Changes {
    Paths(BTreeSet<String>),
    Broad(String),
}

pub(super) fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(format!(
            "git {} failed: {}",
            args.first().copied().unwrap_or_default(),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

pub(super) fn nul_paths(stdout: &[u8]) -> Result<BTreeSet<String>, String> {
    stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|chunk| String::from_utf8(chunk.to_vec()).map_err(|_| NON_UTF8.to_owned()))
        .collect()
}

pub(super) fn sha(rev: &str, role: &str) -> Result<(), String> {
    if rev.starts_with('-') {
        return Err(format!("{role}_leading_dash"));
    }
    if (SHORT_SHA_MIN..=FULL_SHA).contains(&rev.len()) && rev.bytes().all(|b| b.is_ascii_hexdigit())
    {
        Ok(())
    } else {
        Err(format!("{role}_must_be_hex_sha"))
    }
}

pub(super) fn verify_checkout(root: &Path, event: Event, head: &str) -> Result<(), String> {
    if event == Event::Local {
        return Ok(());
    }
    sha(head, "bad_head")?;
    let checkout = rev(root, "HEAD").map_err(|problem| format!("bad_checkout:{problem}"))?;
    if checkout.as_deref() == Some(head) {
        return Ok(());
    }
    if rev(root, "HEAD^2").ok().flatten().as_deref() == Some(head) {
        return Ok(());
    }
    Err("checkout_head_mismatch".to_owned())
}

fn rev(root: &Path, spec: &str) -> Result<Option<String>, String> {
    let Ok(stdout) = git(root, &["rev-parse", spec]) else {
        return if spec == "HEAD" {
            Err("missing_head".to_owned())
        } else {
            Ok(None)
        };
    };
    let text = String::from_utf8(stdout).map_err(|error| error.to_string())?;
    let found = text.trim();
    sha(found, "bad_head")?;
    Ok(Some(found.to_owned()))
}

pub(super) fn find(root: &Path, event: Event, base: Option<&str>, head: &str) -> Changes {
    match find_inner(root, event, base, head) {
        Ok(changes) => changes,
        Err(problem) => Changes::Broad(format!("comparison_unavailable:{problem}")),
    }
}

fn find_inner(
    root: &Path,
    event: Event,
    base: Option<&str>,
    head: &str,
) -> Result<Changes, String> {
    if event == Event::Local {
        let mut changed = dirty(root)?;
        changed.extend(untracked(root)?);
        return Ok(settled(changed));
    }
    let Some(base) = base else {
        return Ok(Changes::Broad(
            "comparison_unavailable:missing_base".to_owned(),
        ));
    };
    let checkout = rev(root, "HEAD")?.unwrap_or_default();
    if checkout != head && rev(root, "HEAD^1")?.as_deref() != Some(base) {
        return Ok(Changes::Broad(
            "comparison_unavailable:stale_merge_ref".to_owned(),
        ));
    }
    sha(base, "bad_base")?;
    sha(head, "bad_head")?;
    let range = format!("{base}...{head}");
    let changed = nul_paths(&git(
        root,
        &["diff", "-z", "--name-only", "--no-renames", &range, "--"],
    )?)?;
    if untracked(root)?.into_iter().any(|path| !advisory(&path)) {
        return Ok(Changes::Broad("untracked_files".to_owned()));
    }
    if dirty(root)?.iter().any(|path| !advisory(path)) {
        return Ok(Changes::Broad("dirty_worktree".to_owned()));
    }
    Ok(settled(changed))
}

fn settled(changed: BTreeSet<String>) -> Changes {
    if changed
        .iter()
        .any(|path| advisory(path) || tool_source(path))
    {
        return Changes::Broad("project_tool_config_changed".to_owned());
    }
    Changes::Paths(changed)
}

fn dirty(root: &Path) -> Result<BTreeSet<String>, String> {
    let mut changed = BTreeSet::new();
    for cached in [true, false] {
        let mut args = vec!["diff", "-z", "--name-only"];
        if cached {
            args.push("--cached");
        }
        args.extend(["--no-renames", "--"]);
        changed.extend(nul_paths(&git(root, &args)?)?);
    }
    Ok(changed)
}

fn untracked(root: &Path) -> Result<BTreeSet<String>, String> {
    nul_paths(&git(
        root,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )?)
}

fn advisory(path: &str) -> bool {
    path == ".mise.toml" || path == "rust-toolchain.toml" || path == "rust-toolchain" || {
        path == "mise.toml" || path == "mise.lock"
    }
}

fn tool_source(path: &str) -> bool {
    PROJECT_TOOL_FILES.contains(&path)
        || MISE_LOCKS.contains(&path)
        || MISE_CONFIGS
            .iter()
            .any(|pattern| source_pattern(pattern, path))
}

fn source_pattern(pattern: &str, path: &str) -> bool {
    let (pattern_dir, pattern_name) = pattern.rsplit_once('/').unwrap_or(("", pattern));
    let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
    if dir != pattern_dir {
        return false;
    }
    match pattern_name.split_once('*') {
        None => name == pattern_name,
        Some((prefix, suffix)) => {
            name.len() >= prefix.len() + suffix.len()
                && name.starts_with(prefix)
                && name.ends_with(suffix)
        }
    }
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::Path;

use velnor_actions_contract::config::project_tool_refs::{
    PROJECT_MISE_CONFIG_PATTERNS, PROJECT_TOOL_VERSIONS_FILE, RUST_TOOLCHAIN_SOURCE_FILES,
    matches_source_pattern,
};
use velnor_actions_mise::{GitRequest, PinnedTool};

use crate::OrchestratorError;
use crate::git_paths::split_nul_paths_skipping;
use crate::safe_read::{MAX_REPO_FILE_BYTES, RepoRead, read_repo_file};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RawDeclaration {
    pub(crate) tool: PinnedTool,
    pub(crate) key: String,
    pub(crate) value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceDeclarations {
    pub(crate) file: String,
    pub(crate) declarations: Vec<RawDeclaration>,
}

pub(crate) fn parse_source(path: &str, text: &str) -> Result<Vec<RawDeclaration>, String> {
    if path == PROJECT_TOOL_VERSIONS_FILE {
        return Ok(tool_versions(text));
    }
    if RUST_TOOLCHAIN_SOURCE_FILES.contains(&path) {
        return toolchain(path, text);
    }
    mise_toml(text)
}

fn mise_toml(text: &str) -> Result<Vec<RawDeclaration>, String> {
    let value: toml::Value =
        toml::from_str(text).map_err(|err| format!("not valid TOML: {err}"))?;
    let Some(tools) = value.get("tools").and_then(toml::Value::as_table) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for (key, value) in tools {
        let Some(tool) = catalog_tool(key) else {
            continue;
        };
        if let Some(value) = raw_value(value) {
            out.push(RawDeclaration {
                tool,
                key: key.clone(),
                value,
            });
        }
    }
    Ok(out)
}

fn raw_value(value: &toml::Value) -> Option<String> {
    match value {
        toml::Value::String(text) => Some(text.clone()),
        toml::Value::Table(table) => table.get("version").map(|version| match version {
            toml::Value::String(text) => text.clone(),
            other => other.to_string(),
        }),
        other => Some(other.to_string()),
    }
}

fn tool_versions(text: &str) -> Vec<RawDeclaration> {
    let mut out = Vec::new();
    for line in text.lines() {
        let code = line.split_once('#').map_or(line, |(code, _)| code);
        let code = code.trim_matches([' ', '\t']);
        if code.is_empty() {
            continue;
        }
        let (key, value) = code
            .split_once([' ', '\t'])
            .map_or((code, ""), |(key, value)| {
                (key, value.trim_matches([' ', '\t']))
            });
        if let Some(tool) = catalog_tool(key) {
            out.push(RawDeclaration {
                tool,
                key: key.to_owned(),
                value: value.to_owned(),
            });
        }
    }
    out
}

fn toolchain(path: &str, text: &str) -> Result<Vec<RawDeclaration>, String> {
    let channel = if path == "rust-toolchain" && !text.trim_start().starts_with('[') {
        Some(text.chars().filter(|c| !" \t\r\n".contains(*c)).collect())
    } else {
        let value: toml::Value =
            toml::from_str(text).map_err(|err| format!("not valid TOML: {err}"))?;
        value
            .get("toolchain")
            .and_then(|toolchain| toolchain.get("channel"))
            .map(|channel| match channel {
                toml::Value::String(text) => text.clone(),
                other => other.to_string(),
            })
    };
    Ok(channel
        .into_iter()
        .map(|value| RawDeclaration {
            tool: PinnedTool::Rust,
            key: "toolchain.channel".to_owned(),
            value,
        })
        .collect())
}

fn catalog_tool(key: &str) -> Option<PinnedTool> {
    PinnedTool::ALL
        .into_iter()
        .filter(|tool| *tool != PinnedTool::Gh)
        .find(|tool| tool.mise_config_keys().contains(&key))
}

pub(crate) fn is_mise_config(rel: &str) -> bool {
    PROJECT_MISE_CONFIG_PATTERNS
        .iter()
        .any(|pattern| matches_source_pattern(pattern, rel))
}

pub(crate) fn source_paths(root: &Path) -> Result<Vec<String>, OrchestratorError> {
    let mut dirs: Vec<&str> = PROJECT_MISE_CONFIG_PATTERNS
        .iter()
        .map(|pattern| pattern.rsplit_once('/').map_or("", |(dir, _)| dir))
        .collect();
    dirs.sort_unstable();
    dirs.dedup();
    let mut paths = Vec::new();
    for dir in dirs {
        let listing = match std::fs::read_dir(root.join(dir)) {
            Ok(listing) => listing,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) if err.kind() == std::io::ErrorKind::NotADirectory => continue,
            Err(err) => {
                return Err(OrchestratorError::config(
                    if dir.is_empty() { "." } else { dir },
                    "tools",
                    format!("project_tool_source_unreadable:{err}"),
                ));
            }
        };
        for entry in listing {
            let entry = entry.map_err(|err| {
                OrchestratorError::config(
                    dir,
                    "tools",
                    format!("project_tool_source_unreadable:{err}"),
                )
            })?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let rel = if dir.is_empty() {
                name
            } else {
                format!("{dir}/{name}")
            };
            if is_mise_config(&rel) {
                paths.push(rel);
            }
        }
    }
    paths.sort();
    paths.dedup();
    paths.push(PROJECT_TOOL_VERSIONS_FILE.to_owned());
    paths.extend(
        RUST_TOOLCHAIN_SOURCE_FILES
            .iter()
            .map(|file| (*file).to_owned()),
    );
    Ok(paths)
}

pub(crate) fn read_project_sources(
    root: &Path,
) -> Result<Vec<SourceDeclarations>, OrchestratorError> {
    let mut sources = Vec::new();
    for file in source_paths(root)? {
        let RepoRead::Text(text) = read_repo_file(root, &file, MAX_REPO_FILE_BYTES)? else {
            continue;
        };
        let declarations = parse_source(&file, &text).map_err(|problem| {
            OrchestratorError::config(
                file.as_str(),
                "tools",
                format!("malformed_project_tool_source:{problem}"),
            )
        })?;
        sources.push(SourceDeclarations { file, declarations });
    }
    Ok(sources)
}

pub(crate) fn read_nested_toolchains(
    root: &Path,
) -> Result<Vec<SourceDeclarations>, OrchestratorError> {
    let mut out = Vec::new();
    for file in nested_toolchain_paths(root) {
        let RepoRead::Text(text) = read_repo_file(root, &file, MAX_REPO_FILE_BYTES)? else {
            continue;
        };
        let name = file.rsplit('/').next().unwrap_or(&file);
        let declarations = parse_source(name, &text).map_err(|problem| {
            OrchestratorError::config(
                file.as_str(),
                "toolchain.channel",
                format!("malformed_project_tool_source:{problem}"),
            )
        })?;
        out.push(SourceDeclarations { file, declarations });
    }
    Ok(out)
}

pub(crate) fn nested_toolchain_paths(root: &Path) -> Vec<String> {
    let listed = [
        ["--cached"].as_slice(),
        ["--others", "--exclude-standard"].as_slice(),
    ]
    .iter()
    .map(|mode| {
        let mut args: Vec<std::ffi::OsString> = vec!["-z".into()];
        args.extend(mode.iter().map(std::ffi::OsString::from));
        args.extend(
            ["--", "*rust-toolchain", "*rust-toolchain.toml"].map(std::ffi::OsString::from),
        );
        let output = GitRequest::ls_files(args).run_in(root).ok()?;
        output
            .success
            .then(|| split_nul_paths_skipping(&output.stdout).0)
    })
    .collect::<Option<Vec<_>>>();
    let mut paths: Vec<String> = match listed {
        Some(lists) => lists.into_iter().flatten().collect(),
        None => walk_toolchains(root, Path::new("")),
    };
    paths.retain(|path| {
        path.rsplit_once('/')
            .is_some_and(|(_, name)| RUST_TOOLCHAIN_SOURCE_FILES.contains(&name))
    });
    paths.sort();
    paths.dedup();
    paths
}

fn walk_toolchains(root: &Path, rel: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(listing) = std::fs::read_dir(root.join(rel)) else {
        return out;
    };
    for entry in listing.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let child = rel.join(&name);
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if !matches!(name.as_str(), ".git" | "target" | "node_modules") {
                out.extend(walk_toolchains(root, &child));
            }
        } else if RUST_TOOLCHAIN_SOURCE_FILES.contains(&name.as_str())
            && let Some(text) = child.to_str()
        {
            out.push(text.to_owned());
        }
    }
    out
}

pub(crate) fn declared_versions(
    path: &str,
    text: &str,
) -> Result<BTreeMap<PinnedTool, Vec<(String, String)>>, String> {
    let mut out: BTreeMap<PinnedTool, Vec<(String, String)>> = BTreeMap::new();
    for declaration in parse_source(path, text)? {
        out.entry(declaration.tool)
            .or_default()
            .push((declaration.key, declaration.value));
    }
    for entries in out.values_mut() {
        entries.sort();
    }
    Ok(out)
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! A Cargo.lock that `exclude` drops is in neither the sources key nor the prefetch, so a job
//! that runs cargo against its manifest fails offline. This finds the excluded roots whose lock
//! is tracked, and the run text that runs cargo in their directory.
//!
//! A `run` step of `.velnor/workflows.toml` is a declared job step, so a literal reference
//! there is a refusal. A mise task or a hand-written workflow file may never run, and its text
//! is read here without a shell or a YAML parser, so a reference there is only a warning.

use velnor_model::{RepoPath, RepoView, RustCiConfig, WORKFLOWS_CONFIG_PATH};

use crate::classify::is_workflow_file;
use crate::config::excluded;
use crate::detect::{config, manifests};
use crate::error::DetectError;
use crate::mise::MISE_CONFIGS;
use crate::roots::CARGO_LOCK;

use shell::segments;
use yaml::yaml_steps;

mod shell;
mod yaml;

/// A cargo run against an excluded root: a refusal for a declared job step, else a note.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExcludedLockFinding {
    pub refusal: bool,
    pub file: RepoPath,
    pub lock: RepoPath,
}

impl ExcludedLockFinding {
    #[must_use]
    pub fn message(&self) -> String {
        if self.refusal {
            format!(
                "{lock} is excluded from the sources key and prefetch, but a job runs cargo against it, so cargo fails offline; remove it from the exclude list in .velnor/rust-ci.toml, or stop running cargo against it in CI",
                lock = self.lock,
            )
        } else {
            format!(
                "{lock} is excluded from the sources key and prefetch, but {file} may run cargo against it; if a job runs that, remove it from the exclude list in .velnor/rust-ci.toml, or stop running cargo against it in CI",
                lock = self.lock,
                file = self.file,
            )
        }
    }
}

/// The cargo runs against excluded roots whose lock is tracked. Declared job steps refuse;
/// mise tasks and hand-written workflows only note.
pub fn excluded_lock_findings(view: &RepoView) -> Result<Vec<ExcludedLockFinding>, DetectError> {
    let config = config(view)?;
    let roots = excluded_roots(view, &config);
    let mut findings: Vec<ExcludedLockFinding> = Vec::new();
    for site in sites(view) {
        for dir in cargo_dirs(&site.run, site.cwd.as_deref()) {
            for root in &roots {
                if !dir.reaches(&root.dir) {
                    continue;
                }
                let finding = ExcludedLockFinding {
                    refusal: site.refusal && matches!(dir, Dir::Literal(_)),
                    file: site.file.clone(),
                    lock: root.lock.clone(),
                };
                if !findings.contains(&finding) {
                    findings.push(finding);
                }
            }
        }
    }
    Ok(findings)
}

struct Excluded {
    /// The directory of the manifest, without a trailing slash; never the repository root.
    dir: String,
    lock: RepoPath,
}

fn excluded_roots(view: &RepoView, config: &RustCiConfig) -> Vec<Excluded> {
    manifests(&view.tracked_paths)
        .into_iter()
        .filter(|manifest| excluded(config, manifest))
        .filter_map(|manifest| {
            let lock = manifest.sibling(&CARGO_LOCK);
            let dir = manifest
                .as_str()
                .strip_suffix("Cargo.toml")?
                .trim_end_matches('/');
            (!dir.is_empty() && view.tracked_paths.contains(&lock)).then(|| Excluded {
                dir: dir.to_owned(),
                lock,
            })
        })
        .collect()
}

/// A run text, or a YAML step's text, with the working directory it starts in.
struct Site {
    file: RepoPath,
    refusal: bool,
    run: String,
    cwd: Option<String>,
}

fn sites(view: &RepoView) -> Vec<Site> {
    let mut sites = Vec::new();
    if let Some(text) = view
        .files
        .get(&WORKFLOWS_CONFIG_PATH)
        .and_then(|b| text_of(b))
    {
        sites.extend(declared_steps(&text).into_iter().map(|(run, cwd)| Site {
            file: WORKFLOWS_CONFIG_PATH,
            refusal: true,
            run,
            cwd,
        }));
    }
    for path in &MISE_CONFIGS {
        let Some(table) = view
            .files
            .get(path)
            .and_then(|b| text_of(b))
            .and_then(|t| toml::from_str::<toml::Table>(&t).ok())
        else {
            continue;
        };
        let Some(tasks) = table.get("tasks").and_then(toml::Value::as_table) else {
            continue;
        };
        for task in tasks.values().filter_map(toml::Value::as_table) {
            let run = match task.get("run") {
                Some(toml::Value::String(run)) => run.clone(),
                Some(toml::Value::Array(items)) => items
                    .iter()
                    .filter_map(toml::Value::as_str)
                    .collect::<Vec<_>>()
                    .join("\n"),
                _ => continue,
            };
            let cwd = task
                .get("dir")
                .and_then(toml::Value::as_str)
                .map(str::to_owned);
            sites.push(Site {
                file: path.clone(),
                refusal: false,
                run,
                cwd,
            });
        }
    }
    for (path, bytes) in &view.files {
        if !is_workflow_file(path) {
            continue;
        }
        let Some(text) = text_of(bytes) else { continue };
        sites.extend(yaml_steps(&text).into_iter().map(|(run, cwd)| Site {
            file: path.clone(),
            refusal: false,
            run,
            cwd,
        }));
    }
    sites
}

fn text_of(bytes: &[u8]) -> Option<String> {
    std::str::from_utf8(bytes).ok().map(str::to_owned)
}

/// The `run` and `working_directory` of each declared step, in each job and case.
fn declared_steps(text: &str) -> Vec<(String, Option<String>)> {
    let Ok(file) = toml::from_str::<toml::Table>(text) else {
        return Vec::new();
    };
    let mut steps = Vec::new();
    for workflow in tables(file.get("workflow")) {
        for unit in ["job", "case"]
            .iter()
            .flat_map(|key| tables(workflow.get(*key)))
        {
            for step in tables(unit.get("step")) {
                let Some(run) = step.get("run").and_then(toml::Value::as_str) else {
                    continue;
                };
                let cwd = step
                    .get("working_directory")
                    .and_then(toml::Value::as_str)
                    .map(str::to_owned);
                steps.push((run.to_owned(), cwd));
            }
        }
    }
    steps
}

fn tables(value: Option<&toml::Value>) -> Vec<&toml::Table> {
    value
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_table)
        .collect()
}

/// Where a cargo command or a path points: a repository-relative directory, or unknown text.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Dir {
    Literal(String),
    Dynamic,
}

impl Dir {
    fn reaches(&self, root: &str) -> bool {
        match self {
            Self::Dynamic => true,
            Self::Literal(dir) => {
                dir == root
                    || dir
                        .strip_prefix(root)
                        .is_some_and(|rest| rest.starts_with('/'))
            }
        }
    }
}

/// The directory each cargo command in a run text is run against: its `--manifest-path`'s
/// directory, or else the directory it starts in.
fn cargo_dirs(run: &str, start: Option<&str>) -> Vec<Dir> {
    let mut cwd = literal(start.unwrap_or(""));
    let mut dirs = Vec::new();
    for words in segments(run) {
        if words.first().map(String::as_str) == Some("cd") {
            cwd = match words.get(1) {
                Some(word) => resolve(&cwd, word),
                None => Dir::Dynamic,
            };
            continue;
        }
        if !words
            .iter()
            .any(|word| word.as_str() == "cargo" || word.ends_with("/cargo"))
        {
            continue;
        }
        let manifest = words.iter().enumerate().find_map(|(index, word)| {
            if word.as_str() == "--manifest-path" {
                words.get(index + 1).cloned()
            } else {
                word.strip_prefix("--manifest-path=").map(str::to_owned)
            }
        });
        match manifest {
            Some(path) => dirs.push(match resolve(&cwd, &path) {
                Dir::Literal(path) => Dir::Literal(match path.strip_suffix("Cargo.toml") {
                    Some(dir) => dir.trim_end_matches('/').to_owned(),
                    None => path,
                }),
                Dir::Dynamic => Dir::Dynamic,
            }),
            None => dirs.push(cwd.clone()),
        }
    }
    dirs
}

/// `word` as a path, relative to `base` unless it names the workspace or is absolute.
fn resolve(base: &Dir, word: &str) -> Dir {
    let workspace = word
        .strip_prefix("$GITHUB_WORKSPACE/")
        .or_else(|| word.strip_prefix("${GITHUB_WORKSPACE}/"));
    if workspace.is_some() || word.starts_with('/') {
        return literal(workspace.unwrap_or(word));
    }
    match base {
        Dir::Literal(base) if base.is_empty() => literal(word),
        Dir::Literal(base) => literal(&format!("{base}/{word}")),
        Dir::Dynamic => Dir::Dynamic,
    }
}

/// A path with `.` and `..` resolved; any `$`, backtick or absolute path is unknown.
fn literal(path: &str) -> Dir {
    if path == "$GITHUB_WORKSPACE" || path == "${GITHUB_WORKSPACE}" {
        return Dir::Literal(String::new());
    }
    if path.contains(['$', '`']) || path.starts_with('/') {
        return Dir::Dynamic;
    }
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    Dir::Literal(parts.join("/"))
}

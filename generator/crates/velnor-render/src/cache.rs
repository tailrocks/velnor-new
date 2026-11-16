// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{CacheKey, CachePath, RepoPath};

use crate::steps::{MISE_VERSION, PREPARE_STEP};

const HOST: &str = "${{ runner.os }}-${{ runner.arch }}";
const COMMIT: &str = "${{ github.sha }}";
const RUN_ID: &str = "${{ github.run_id }}";
const DELTA_BASE: &str = "${{ steps.restored.outputs.base8 }}";
const MISE_LOCK: RepoPath = RepoPath::from_static("mise.lock");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedKey {
    pub key: String,
    pub restore_keys: Vec<String>,
}

struct Layout<'a> {
    segments: Vec<&'a str>,
    fallbacks: &'static [usize],
}

#[must_use]
pub fn render_key(key: &CacheKey) -> RenderedKey {
    render_key_with_mise(key, MISE_VERSION)
}

/// The tools entry is saved only on a miss, so the mise program version that
/// `jdx/mise-action` installs must be part of the key.
#[must_use]
pub fn render_key_with_mise(key: &CacheKey, mise_version: &str) -> RenderedKey {
    let mise = format!("mise{mise_version}");
    let compat = format!("${{{{ steps.{PREPARE_STEP}.outputs.compat }}}}");
    let lock = match key {
        CacheKey::Tools { .. } => hash_files(&MISE_LOCK),
        CacheKey::Sources { root, .. } | CacheKey::Target { root, .. } => {
            hash_files(&root.lock_file())
        }
        CacheKey::Delta { .. } => String::new(),
    };
    let owner;
    let layout = match key {
        CacheKey::Tools { .. } => Layout {
            segments: vec!["velnor", "tools", HOST, &mise, &lock],
            fallbacks: &[],
        },
        CacheKey::Sources { root, .. } => Layout {
            segments: vec!["velnor", "src", HOST, root.slug().as_str(), &lock],
            fallbacks: &[4],
        },
        CacheKey::Target { job, root, .. } => {
            owner = format!("{job}.{}.{HOST}", root.slug());
            Layout {
                segments: vec!["velnor", "tgt", &owner, &compat, &lock, COMMIT, RUN_ID],
                fallbacks: &[5, 4],
            }
        }
        CacheKey::Delta { job, root, .. } => Layout {
            segments: vec![
                "velnor",
                "dlt",
                job.as_str(),
                root.slug().as_str(),
                HOST,
                &compat,
                DELTA_BASE,
                RUN_ID,
            ],
            fallbacks: &[7],
        },
    };
    joined(&layout)
}

fn joined(layout: &Layout<'_>) -> RenderedKey {
    RenderedKey {
        key: layout.segments.join("-"),
        restore_keys: layout
            .fallbacks
            .iter()
            .map(|length| format!("{}-", layout.segments[..*length].join("-")))
            .collect(),
    }
}

fn hash_files(path: &RepoPath) -> String {
    format!(
        "${{{{ hashFiles({}) }}}}",
        string_expression(&glob_literal(path.as_str()))
    )
}

pub(crate) fn glob_literal(text: &str) -> String {
    let escaped: String = text
        .chars()
        .flat_map(|c| match c {
            '*' => vec!['[', '*', ']'],
            '?' => vec!['[', '?', ']'],
            '[' => vec!['[', '[', ']'],
            other => vec![other],
        })
        .collect();
    if escaped.starts_with(['!', '#']) {
        format!("./{escaped}")
    } else {
        escaped
    }
}

fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

fn string_expression(text: &str) -> String {
    if !text.contains(['{', '}']) {
        return quoted(text);
    }
    let mut arguments = Vec::new();
    let mut run = String::new();
    for c in text.chars() {
        if matches!(c, '{' | '}') {
            if !run.is_empty() {
                arguments.push(quoted(&std::mem::take(&mut run)));
            }
            arguments.push(format!("fromJSON('\"\\u{:04x}\"')", u32::from(c)));
        } else {
            run.push(c);
        }
    }
    if !run.is_empty() {
        arguments.push(quoted(&run));
    }
    let slots = (0..arguments.len())
        .map(|index| format!("{{{index}}}"))
        .collect::<Vec<_>>()
        .concat();
    format!("format('{slots}', {})", arguments.join(", "))
}

pub(crate) fn path_text(path: &CachePath) -> String {
    match path {
        CachePath::MiseData => "~/.local/share/mise".to_owned(),
        CachePath::RustupHome => "~/.rustup".to_owned(),
        CachePath::CargoRegistryIndex => "~/.cargo/registry/index".to_owned(),
        CachePath::CargoRegistryCache => "~/.cargo/registry/cache".to_owned(),
        CachePath::CargoGitDb => "~/.cargo/git/db".to_owned(),
        CachePath::Target(directory) => directory.to_string(),
    }
}

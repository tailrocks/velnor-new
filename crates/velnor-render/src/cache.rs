// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{CacheKey, CacheKeySegment, CachePath, LOCK_OUTPUT_PREFIX, RepoPath};

use crate::gate::SOURCES_LOCKS_STEP;
use crate::rules::Value;
use crate::steps::{MISE_VERSION, PREPARE_STEP};

const HOST: &str = "${{ runner.os }}-${{ runner.arch }}";
const COMMIT: &str = "${{ github.sha }}";
const RUN_ID: &str = "${{ github.run_id }}";
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedKey {
    pub key: String,
    pub restore_keys: Vec<String>,
}

#[must_use]
pub fn render_key(key: &CacheKey) -> RenderedKey {
    render_key_with_mise(key, MISE_VERSION)
}

#[must_use]
pub fn render_key_with_mise(key: &CacheKey, mise_version: &str) -> RenderedKey {
    let layout = key.layout();
    let segments: Vec<String> = layout
        .segments
        .iter()
        .map(|segment| match segment {
            CacheKeySegment::Text(text) => text.clone(),
            CacheKeySegment::Host => HOST.to_owned(),
            CacheKeySegment::TargetOwner { job, root } => format!("{job}.{root}.{HOST}"),
            CacheKeySegment::Compat => format!("${{{{ steps.{PREPARE_STEP}.outputs.compat }}}}"),
            CacheKeySegment::Locks(paths) => hash_files(paths),
            CacheKeySegment::RootLock(root) => {
                format!("${{{{ steps.{PREPARE_STEP}.outputs.{LOCK_OUTPUT_PREFIX}{root} }}}}")
            }
            CacheKeySegment::SourcesLocks => {
                format!("${{{{ steps.{SOURCES_LOCKS_STEP}.outputs.hash }}}}")
            }
            CacheKeySegment::Mise => format!("mise{mise_version}"),
            CacheKeySegment::Commit => COMMIT.to_owned(),
            CacheKeySegment::RunId => RUN_ID.to_owned(),
            CacheKeySegment::PullRequest => "pr${{ github.event.pull_request.number }}".to_owned(),
            CacheKeySegment::DeltaBase(root) => {
                format!("${{{{ steps.delta_base_{root}.outputs.base8 }}}}")
            }
        })
        .collect();
    RenderedKey {
        key: segments.join("-"),
        restore_keys: layout
            .fallback_lengths
            .iter()
            .map(|&length| format!("{}-", segments[..length].join("-")))
            .collect(),
    }
}

pub(crate) fn hash_files(paths: &[RepoPath]) -> String {
    let patterns: Vec<String> = paths
        .iter()
        .map(|path| string_expression(&glob_literal(path.as_str())))
        .collect();
    format!("${{{{ hashFiles({}) }}}}", patterns.join(", "))
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
    if escaped.starts_with(['!', '#', '~']) {
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
        CachePath::Target(directory) => glob_literal(directory.as_str()),
        CachePath::Delta(root) => root.delta_dir(),
    }
}

pub(crate) fn action_path(text: &str) -> Value {
    if !text.contains("${{") {
        return Value::Text(text.to_owned());
    }
    let escaped = text
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
        .replace('\'', "''")
        .replace("${{", "\\u0024{{");
    Value::Expr(format!("${{{{ fromJSON('\"{escaped}\"') }}}}"))
}

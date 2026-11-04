// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::path::Path;

use velnor_actions_workflow_renderer::render::RenderedTree;

use super::support::rendered_job_block;

pub(super) type Files = BTreeMap<String, String>;

pub(super) const AUTO_INSTALL_ENV: &str = "RUSTUP_AUTO_INSTALL";
pub(super) const CLEAN_CWD_JOBS: [&str; 1] = ["cargo-deny"];
const TOOLCHAIN_WORDS: [&str; 4] = ["cargo", "rustup", "rustc", "mbx"];
const SEPARATORS: [char; 10] = [';', '&', '|', '(', ')', '\'', '"', '\\', '<', '>'];
const ACTION_PREFIX: &str = "./.github/actions/";
const ACTION_DEPTH: usize = 3;

#[derive(Debug)]
pub(super) struct JobUse {
    pub(super) file: String,
    pub(super) job: String,
    pub(super) words: BTreeSet<&'static str>,
    pub(super) installs_rust: bool,
    pub(super) env_off: bool,
}

impl JobUse {
    pub(super) fn is_user(&self) -> bool {
        self.installs_rust || !self.words.is_empty()
    }
}

pub(super) fn files_of(tree: &RenderedTree) -> Result<Files, Box<dyn Error>> {
    tree.paths()
        .into_iter()
        .map(|path| {
            let bytes = tree.get(&path).ok_or("rendered file vanished")?.to_owned();
            Ok((path, bytes))
        })
        .collect()
}

pub(super) fn files_of_repo(root: &Path) -> Result<Files, Box<dyn Error>> {
    let mut files = Files::new();
    let workflows = root.join(".github/workflows");
    for entry in fs::read_dir(&workflows)? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "yml")
            && let Some(name) = path.file_name().and_then(|name| name.to_str())
        {
            files.insert(
                format!(".github/workflows/{name}"),
                fs::read_to_string(&path)?,
            );
        }
    }
    for entry in fs::read_dir(root.join(".github/actions"))? {
        let dir = entry?.path();
        let action = dir.join("action.yml");
        if let (true, Some(name)) = (
            action.is_file(),
            dir.file_name().and_then(|name| name.to_str()),
        ) {
            files.insert(
                format!(".github/actions/{name}/action.yml"),
                fs::read_to_string(&action)?,
            );
        }
    }
    Ok(files)
}

pub(super) fn children(text: &str) -> Vec<(String, String)> {
    let mut lines = text.lines();
    let head = lines
        .next()
        .map_or(0, |line| line.len() - line.trim_start().len());
    lines
        .take_while(|line| line.len() - line.trim_start().len() > head)
        .filter_map(|line| line.trim().split_once(": "))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect()
}

pub(super) fn job_env_entries(
    workflow: &str,
    block: &str,
) -> Result<Vec<(String, String)>, Box<dyn Error>> {
    let Some(at) = block.find("\n    env:") else {
        return Ok(Vec::new());
    };
    let from_head = &block[at + 1..];
    let value = from_head
        .lines()
        .next()
        .and_then(|line| line.trim().strip_prefix("env:"))
        .map_or("", str::trim);
    let Some(alias) = value.strip_prefix('*') else {
        return Ok(children(from_head));
    };
    let marker = format!("env: &{alias}\n");
    let found = workflow
        .find(&marker)
        .ok_or_else(|| format!("env anchor {alias} is not defined"))?;
    let start = workflow[..found]
        .rfind('\n')
        .map_or(0, |newline| newline + 1);
    Ok(children(&workflow[start..]))
}

pub(super) fn job_ids(workflow: &str) -> Vec<&str> {
    workflow
        .lines()
        .skip_while(|line| *line != "jobs:")
        .skip(1)
        .filter(|line| line.starts_with("  ") && !line.starts_with("   "))
        .filter_map(|line| line.trim().strip_suffix(':'))
        .collect()
}

fn decode_double_quoted(raw: &str) -> String {
    let mut decoded = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            decoded.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => decoded.push('\n'),
            Some('t') => decoded.push('\t'),
            Some('"') => decoded.push('"'),
            None | Some('\\') => decoded.push('\\'),
            Some(other) => {
                decoded.push('\\');
                decoded.push(other);
            }
        }
    }
    decoded
}

fn double_quoted_body(value: &str) -> Result<&str, Box<dyn Error>> {
    let body = value
        .strip_prefix('"')
        .ok_or("not a double-quoted scalar")?;
    let mut escaped = false;
    for (at, c) in body.char_indices() {
        match (escaped, c) {
            (true, _) => escaped = false,
            (false, '\\') => escaped = true,
            (false, '"') => return Ok(&body[..at]),
            _ => {}
        }
    }
    Err("run scalar spans several lines".into())
}

pub(super) fn run_scalar(
    lines: &[&str],
    at: usize,
    value: &str,
    label: &str,
) -> Result<String, Box<dyn Error>> {
    let value = match value.strip_prefix('&') {
        Some(anchored) => anchored.split_once(' ').map_or("", |(_, rest)| rest),
        None => value,
    };
    if value.starts_with('*') {
        return Err(format!("{label}: unresolved run alias {value}").into());
    }
    if value.starts_with('"') {
        return Ok(decode_double_quoted(
            double_quoted_body(value).map_err(|error| format!("{label}: {error}"))?,
        ));
    }
    if value.starts_with('|') || value.starts_with('>') {
        let head = lines[at].len() - lines[at].trim_start().len();
        return Ok(lines[at + 1..]
            .iter()
            .take_while(|line| {
                line.trim().is_empty() || line.len() - line.trim_start().len() > head
            })
            .copied()
            .collect::<Vec<_>>()
            .join("\n"));
    }
    Ok(value.trim_matches('\'').to_owned())
}

fn scalars_of(text: &str, label: &str) -> Result<(Vec<String>, Vec<String>), Box<dyn Error>> {
    let lines: Vec<&str> = text.lines().collect();
    let mut scalars = Vec::new();
    let mut actions = Vec::new();
    for (at, line) in lines.iter().enumerate() {
        let trimmed = line.trim().trim_start_matches("- ");
        if line.trim().starts_with("- *") {
            return Err(format!("{label}: unresolved step alias {}", line.trim()).into());
        }
        if let Some(value) = trimmed.strip_prefix("run:") {
            scalars.push(run_scalar(&lines, at, value.trim(), label)?);
        } else if let Some(value) = trimmed.strip_prefix("uses:")
            && let Some(name) = value
                .split_whitespace()
                .next()
                .and_then(|uses| uses.strip_prefix(ACTION_PREFIX))
        {
            actions.push(name.to_owned());
        }
    }
    Ok((scalars, actions))
}

fn job_scalars(
    files: &Files,
    block: &str,
    label: &str,
    depth: usize,
) -> Result<Vec<String>, Box<dyn Error>> {
    let (mut scalars, actions) = scalars_of(block, label)?;
    if depth == 0 {
        return Ok(scalars);
    }
    for name in actions {
        let path = format!(".github/actions/{name}/action.yml");
        let text = files
            .get(&path)
            .ok_or_else(|| format!("{label}: local action {path} is missing"))?;
        scalars.extend(job_scalars(files, text, &path, depth - 1)?);
    }
    Ok(scalars)
}

fn tokens(scalar: &str) -> impl Iterator<Item = &str> {
    scalar
        .split(|c: char| c.is_whitespace() || SEPARATORS.contains(&c))
        .filter(|token| !token.is_empty())
}

pub(super) fn file_tokens(files: &Files, path: &str) -> Result<BTreeSet<String>, Box<dyn Error>> {
    let text = files
        .get(path)
        .ok_or_else(|| format!("{path} is not rendered"))?;
    let (scalars, _) = scalars_of(text, path)?;
    Ok(scalars
        .iter()
        .flat_map(|scalar| tokens(scalar))
        .map(str::to_owned)
        .collect())
}

fn installs_lockfile(words: &[&str]) -> bool {
    words.iter().enumerate().any(|(at, word)| {
        let rest = &words[at + 1..];
        let flags = rest.iter().take_while(|token| token.starts_with("--"));
        *word == "mise"
            && flags.clone().any(|flag| *flag == "--locked")
            && rest.get(flags.count()) == Some(&"install")
    })
}

pub(super) fn installs_toolchain(scalar: &str) -> bool {
    let words: Vec<&str> = tokens(scalar).collect();
    words
        .iter()
        .any(|token| token.starts_with("rust@") || token.starts_with("rust["))
        || installs_lockfile(&words)
}

fn scan_job(files: &Files, file: &str, workflow: &str, id: &str) -> Result<JobUse, Box<dyn Error>> {
    let label = format!("{file} {id}");
    let block = rendered_job_block(workflow, id)?;
    let scalars = job_scalars(files, block, &label, ACTION_DEPTH)?;
    let words = scalars
        .iter()
        .flat_map(|scalar| tokens(scalar))
        .filter_map(|token| TOOLCHAIN_WORDS.iter().copied().find(|word| *word == token))
        .collect();
    let installs_rust = scalars.iter().any(|scalar| installs_toolchain(scalar));
    let entries = job_env_entries(workflow, block)?;
    let env_off = entries.iter().any(|(key, value)| {
        key == AUTO_INSTALL_ENV
            && serde_saphyr::from_str::<serde_json::Value>(value)
                .is_ok_and(|scalar| scalar.as_str() == Some("0"))
    });
    Ok(JobUse {
        file: file.to_owned(),
        job: id.to_owned(),
        words,
        installs_rust,
        env_off,
    })
}

pub(super) fn scan(
    files: &Files,
    include: impl Fn(&str) -> bool,
) -> Result<Vec<JobUse>, Box<dyn Error>> {
    let mut jobs = Vec::new();
    for (file, workflow) in files {
        if !file.starts_with(".github/workflows/") || !include(file) {
            continue;
        }
        for id in job_ids(workflow) {
            jobs.push(scan_job(files, file, workflow, id)?);
        }
    }
    Ok(jobs)
}

pub(super) fn violations(jobs: &[JobUse]) -> Vec<String> {
    let mut found = Vec::new();
    for job in jobs.iter().filter(|job| job.is_user()) {
        if CLEAN_CWD_JOBS.contains(&job.job.as_str()) && !job.installs_rust {
            continue;
        }
        let mut reasons: Vec<String> = Vec::new();
        if job.installs_rust {
            reasons.push("installs rust".to_owned());
        }
        reasons.extend(job.words.iter().map(|word| format!("word {word}")));
        if !job.env_off {
            found.push(format!(
                "{} {}: {}, no job env {AUTO_INSTALL_ENV}=0",
                job.file,
                job.job,
                reasons.join(", ")
            ));
        }
        if !job.installs_rust {
            found.push(format!(
                "{} {}: {} with no rust install, and the job is not a clean-cwd job",
                job.file,
                job.job,
                reasons.join(", ")
            ));
        }
    }
    found
}

pub(super) fn find<'a>(jobs: &'a [JobUse], id: &str) -> Result<&'a JobUse, Box<dyn Error>> {
    jobs.iter()
        .find(|job| job.job == id)
        .ok_or_else(|| format!("job {id} is missing from the scanned workflows").into())
}

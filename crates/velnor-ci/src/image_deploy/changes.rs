// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;

use crate::error::CiError;
use crate::github::{GitHub, Repo, parse, pull_files_path};

const PAGE: usize = 100;
const LISTED_AT_MOST: u32 = 30;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub images: Vec<(String, bool)>,
    pub report: String,
}

#[derive(Deserialize)]
struct PullFile {
    filename: String,
    previous_filename: Option<String>,
}

#[must_use]
pub fn glob_matches(pattern: &str, path: &str) -> bool {
    let pattern: Vec<&str> = pattern.split('/').collect();
    let path: Vec<&str> = path.split('/').collect();
    segments(&pattern, &path)
}

fn segments(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((&"**", rest)) => (0..=path.len()).any(|skip| segments(rest, &path[skip..])),
        Some((head, rest)) => path
            .split_first()
            .is_some_and(|(first, remainder)| word(head, first) && segments(rest, remainder)),
    }
}

fn word(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    wildcard(&pattern, &text)
}

fn wildcard(pattern: &[char], text: &[char]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some(('*', rest)) => (0..=text.len()).any(|skip| wildcard(rest, &text[skip..])),
        Some(('?', rest)) => text
            .split_first()
            .is_some_and(|(_, remainder)| wildcard(rest, remainder)),
        Some((head, rest)) => text
            .split_first()
            .is_some_and(|(first, remainder)| first == head && wildcard(rest, remainder)),
    }
}

fn changed_files(
    github: &dyn GitHub,
    repo: &Repo,
    number: u64,
) -> Result<Option<Vec<String>>, CiError> {
    let mut names = Vec::new();
    for page in 1..=LISTED_AT_MOST {
        let files: Vec<PullFile> = parse(
            &github.get(&pull_files_path(repo, number, page))?,
            "pull request files",
        )?;
        let last = files.len() < PAGE;
        for file in files {
            names.push(file.filename);
            names.extend(file.previous_filename);
        }
        if last {
            return Ok(Some(names));
        }
    }
    Ok(None)
}

pub fn parse_matches(lines: &[String]) -> Result<Vec<(String, String)>, CiError> {
    let mut matches = Vec::new();
    for line in lines.iter().map(|line| line.trim_end_matches('\r')) {
        if line.trim().is_empty() {
            continue;
        }
        match line.split_once('=') {
            Some((image, glob)) if !image.is_empty() && !glob.is_empty() => {
                matches.push((image.to_owned(), glob.to_owned()));
            }
            _ => {
                return Err(CiError::new(format!("`{line}` is not IMAGE=GLOB")));
            }
        }
    }
    if matches.is_empty() {
        return Err(CiError::new("name at least one IMAGE=GLOB pattern"));
    }
    Ok(matches)
}

pub fn select(
    github: &dyn GitHub,
    repo: &Repo,
    event: &str,
    pull_request: Option<u64>,
    matches: &[(String, String)],
) -> Result<Selection, CiError> {
    let mut images: Vec<(String, bool)> = Vec::new();
    for (image, _) in matches {
        if !images.iter().any(|(name, _)| name == image) {
            images.push((image.clone(), false));
        }
    }
    let everything = |images: Vec<(String, bool)>, why: &str| Selection {
        images: images.into_iter().map(|(name, _)| (name, true)).collect(),
        report: format!("{why}; every image is selected\n"),
    };
    if event != "pull_request" {
        return Ok(everything(images, &format!("the event is {event}")));
    }
    let number = pull_request
        .ok_or_else(|| CiError::new("--pull-request is required for a pull_request event"))?;
    let Some(files) = changed_files(github, repo, number)? else {
        return Ok(everything(
            images,
            "the pull request changes too many files to list",
        ));
    };
    for (name, selected) in &mut images {
        *selected = files.iter().any(|file| {
            matches
                .iter()
                .any(|(image, glob)| image == name && glob_matches(glob, file))
        });
    }
    let report = images
        .iter()
        .map(|(name, selected)| {
            format!(
                "image {name}: {}",
                if *selected { "selected" } else { "not reached" }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Selection {
        images,
        report: report + "\n",
    })
}

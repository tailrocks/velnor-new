// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/command_files.rs

use std::path::{Path, PathBuf};

use uuid::Uuid;

use super::mask::Masker;
use super::{Annotation, AnnotationLevel, ExecError};

const DIRECTORY: &str = "_runner_file_commands";
const SUMMARY_DIRECTORY: &str = "step_summary";
const MAX_SUMMARY_BYTES: usize = 1 << 20;

#[derive(Debug)]
pub(super) struct FileCommands {
    env: PathBuf,
    output: PathBuf,
    path: PathBuf,
    summary: PathBuf,
}

#[derive(Debug, Default)]
pub(super) struct Collected {
    pub(super) env: Vec<(String, String)>,
    pub(super) outputs: Vec<(String, String)>,
    pub(super) path: Vec<String>,
    pub(super) summary: String,
}

async fn create_file(directory: &Path, kind: &str) -> Result<PathBuf, ExecError> {
    let path = directory.join(format!("{kind}_{}", Uuid::new_v4()));
    tokio::fs::write(&path, b"")
        .await
        .map_err(|source| ExecError::io(format!("create {}", path.display()), source))?;
    Ok(path)
}

async fn read_text(path: &Path) -> Result<String, ExecError> {
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|source| ExecError::io(format!("read {}", path.display()), source))?;
    let _ = tokio::fs::remove_file(path).await;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

impl FileCommands {
    pub(super) async fn create(temp: &Path) -> Result<Self, ExecError> {
        let directory = temp.join(DIRECTORY);
        tokio::fs::create_dir_all(&directory)
            .await
            .map_err(|source| ExecError::io(format!("create {}", directory.display()), source))?;
        Ok(Self {
            env: create_file(&directory, "env").await?,
            output: create_file(&directory, "output").await?,
            path: create_file(&directory, "path").await?,
            summary: create_file(&directory, "summary").await?,
        })
    }

    pub(super) fn variables(&self) -> [(&'static str, String); 4] {
        [
            ("GITHUB_ENV", self.env.to_string_lossy().into_owned()),
            ("GITHUB_OUTPUT", self.output.to_string_lossy().into_owned()),
            ("GITHUB_PATH", self.path.to_string_lossy().into_owned()),
            (
                "GITHUB_STEP_SUMMARY",
                self.summary.to_string_lossy().into_owned(),
            ),
        ]
    }

    pub(super) async fn collect(self) -> Result<Collected, ExecError> {
        Ok(Collected {
            env: parse(&read_text(&self.env).await?)?,
            outputs: parse(&read_text(&self.output).await?)?,
            path: read_text(&self.path)
                .await?
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect(),
            summary: read_text(&self.summary).await?,
        })
    }
}

pub(super) async fn save_summary(
    temp: &Path,
    number: u32,
    summary: &str,
    masker: &Masker,
    annotations: &mut Vec<Annotation>,
) -> Result<Option<PathBuf>, ExecError> {
    if summary.trim().is_empty() {
        return Ok(None);
    }
    if summary.len() > MAX_SUMMARY_BYTES {
        annotations.push(Annotation::new(
            AnnotationLevel::Error,
            format!("Step summary exceeds the {MAX_SUMMARY_BYTES} byte limit and was dropped."),
        ));
        return Ok(None);
    }
    let directory = temp.join(SUMMARY_DIRECTORY);
    tokio::fs::create_dir_all(&directory)
        .await
        .map_err(|source| ExecError::io(format!("create {}", directory.display()), source))?;
    let path = directory.join(format!("{number}.md"));
    let body: String = summary
        .split_inclusive('\n')
        .map(|line| masker.mask(line))
        .collect();
    tokio::fs::write(&path, body)
        .await
        .map_err(|source| ExecError::io(format!("write {}", path.display()), source))?;
    Ok(Some(path))
}

fn entry(name: &str, value: &str, line: &str) -> Result<(String, String), ExecError> {
    if name.is_empty() {
        return Err(ExecError::FileCommand(format!(
            "Invalid format '{line}'. Name must not be empty"
        )));
    }
    Ok((name.to_owned(), value.to_owned()))
}

fn heredoc(
    contents: &str,
    index: &mut usize,
    name: &str,
    delimiter: &str,
    line: &str,
) -> Result<(String, String), ExecError> {
    if name.is_empty() || delimiter.is_empty() {
        return Err(ExecError::FileCommand(format!(
            "Invalid format '{line}'. Name must not be empty and delimiter must not be empty"
        )));
    }
    let start = *index;
    let mut end = *index;
    loop {
        let Some((body, newline)) = read_line(contents, index) else {
            return Err(ExecError::FileCommand(format!(
                "Invalid value. Matching delimiter not found '{delimiter}'"
            )));
        };
        if body == delimiter {
            return Ok((name.to_owned(), contents[start..end].to_owned()));
        }
        if !newline {
            return Err(ExecError::FileCommand(
                "Invalid value. EOF marker missing new line.".to_owned(),
            ));
        }
        end = *index - 1;
    }
}

fn read_line<'a>(contents: &'a str, index: &mut usize) -> Option<(&'a str, bool)> {
    if *index >= contents.len() {
        return None;
    }
    let start = *index;
    if let Some(offset) = contents[start..].find('\n') {
        *index = start + offset + 1;
        Some((&contents[start..start + offset], true))
    } else {
        *index = contents.len();
        Some((&contents[start..], false))
    }
}

fn parse(contents: &str) -> Result<Vec<(String, String)>, ExecError> {
    let contents = contents.strip_prefix('\u{feff}').unwrap_or(contents);
    let mut commands = Vec::new();
    let mut index = 0;
    while let Some((line, _)) = read_line(contents, &mut index) {
        if line.is_empty() {
            continue;
        }
        let equals = line.find('=');
        let marker = line.find("<<");
        if let (Some(equals), Some(marker)) = (equals, marker)
            && equals < marker
        {
            commands.push(entry(&line[..equals], &line[equals + 1..], line)?);
        } else if let Some((name, delimiter)) = line.split_once("<<") {
            commands.push(heredoc(contents, &mut index, name, delimiter, line)?);
        } else if let Some((name, value)) = line.split_once('=') {
            commands.push(entry(name, value, line)?);
        } else {
            return Err(ExecError::FileCommand(format!("Invalid format '{line}'")));
        }
    }
    Ok(commands)
}

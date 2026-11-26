// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/command_files.rs

use std::path::{Path, PathBuf};

use rustix::fs::OFlags;
use tokio::io::AsyncReadExt;
use uuid::Uuid;

use super::lines::split_lines;
use super::output::Output;
use super::private;
use super::{AnnotationLevel, ExecError};

const DIRECTORY: &str = "_runner_file_commands";
const SUMMARY_DIRECTORY: &str = "step_summary";
const FILE_LIMIT: usize = 1 << 20;

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
    pub(super) failures: Vec<String>,
}

async fn create_file(directory: &Path, kind: &str) -> Result<PathBuf, ExecError> {
    let path = directory.join(format!("{kind}_{}", Uuid::new_v4()));
    private::write_new(&path, b"").await?;
    Ok(path)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Command,
    Summary,
}

fn invalid(message: String) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message)
}

async fn read_bounded(path: &Path, kind: Kind) -> std::io::Result<Vec<u8>> {
    let file = tokio::fs::OpenOptions::new()
        .read(true)
        .custom_flags(OFlags::NONBLOCK.bits().cast_signed())
        .open(path)
        .await?;
    let metadata = file.metadata().await?;
    if kind == Kind::Summary && metadata.len() == 0 {
        return Ok(Vec::new());
    }
    if !metadata.is_file() {
        return Err(invalid("not a regular file".to_owned()));
    }
    let mut bytes = Vec::new();
    file.take(FILE_LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .await?;
    if kind == Kind::Command && bytes.len() > FILE_LIMIT {
        return Err(invalid(format!("larger than {FILE_LIMIT} bytes")));
    }
    Ok(bytes)
}

async fn read_text(path: &Path, kind: Kind) -> Result<String, ExecError> {
    let read = read_bounded(path, kind).await;
    let _ = tokio::fs::remove_file(path).await;
    match read {
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes);
            Ok(text.strip_prefix('\u{feff}').unwrap_or(&text).to_owned())
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(source) => Err(ExecError::io(format!("read {}", path.display()), source)),
    }
}

pub(super) fn failure(context: &str, error: &ExecError) -> [String; 2] {
    [
        format!("Unable to process file command '{context}' successfully."),
        error.to_string(),
    ]
}

async fn pairs(path: &Path, context: &str, failures: &mut Vec<String>) -> Vec<(String, String)> {
    let text = match read_text(path, Kind::Command).await {
        Ok(text) => text,
        Err(error) => {
            failures.extend(failure(context, &error));
            return Vec::new();
        }
    };
    let (commands, error) = parse(&text);
    if let Some(error) = error {
        failures.extend(failure(context, &error));
    }
    commands
}

impl FileCommands {
    pub(super) async fn create(temp: &Path) -> Result<Self, ExecError> {
        let directory = temp.join(DIRECTORY);
        private::create_dir(&directory).await?;
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

    pub(super) async fn collect(self) -> Collected {
        let mut failures = Vec::new();
        let env = pairs(&self.env, "env", &mut failures).await;
        let outputs = pairs(&self.output, "output", &mut failures).await;
        let path = match read_text(&self.path, Kind::Command).await {
            Ok(text) => split_lines(&text)
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect(),
            Err(error) => {
                failures.extend(failure("path", &error));
                Vec::new()
            }
        };
        let summary = match read_text(&self.summary, Kind::Summary).await {
            Ok(text) => text,
            Err(error) => {
                failures.extend(failure("step_summary", &error));
                String::new()
            }
        };
        Collected {
            env,
            outputs,
            path,
            summary,
            failures,
        }
    }
}

pub(super) async fn save_summary(
    temp: &Path,
    summary: &str,
    out: &mut Output<'_>,
) -> Result<Option<PathBuf>, ExecError> {
    if summary.trim().is_empty() {
        return Ok(None);
    }
    if summary.len() > FILE_LIMIT {
        out.annotate(
            AnnotationLevel::Error,
            format!("Step summary exceeds the {FILE_LIMIT} byte limit and was dropped."),
        );
        return Ok(None);
    }
    let directory = temp.join(SUMMARY_DIRECTORY);
    private::create_dir(&directory).await?;
    let path = directory.join(format!("{}.md", out.number));
    let body: String = summary
        .split_inclusive('\n')
        .map(|line| out.masker.mask(line))
        .collect();
    private::write_replacing(&path, body.as_bytes()).await?;
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

fn parse(contents: &str) -> (Vec<(String, String)>, Option<ExecError>) {
    let mut commands = Vec::new();
    let error = parse_into(contents, &mut commands).err();
    (commands, error)
}

fn parse_into(contents: &str, commands: &mut Vec<(String, String)>) -> Result<(), ExecError> {
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
    Ok(())
}

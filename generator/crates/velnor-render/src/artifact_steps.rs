// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::rules::{RenderedStep, Value};
use crate::steps::{DOWNLOAD_ARTIFACT, UPLOAD_ARTIFACT};
use velnor_model::RepoPath;

const CARGO_LOG_ARTIFACT: &str = "velnor-cargo-log";
const RUN_LOG_FILE: &str = "velnor-run.log";
const CARGO_LOG_RETENTION_DAYS: u64 = 3;
const ARCHIVE_RETENTION_DAYS: u64 = 7;

pub(crate) fn upload_cargo_log() -> RenderedStep {
    RenderedStep {
        name: "Upload the cargo fingerprint log".to_owned(),
        uses: Some(UPLOAD_ARTIFACT.to_owned()),
        with: vec![
            (
                "name".to_owned(),
                Value::Expr(format!("{CARGO_LOG_ARTIFACT}-${{{{ github.job }}}}")),
            ),
            (
                "path".to_owned(),
                Value::Expr(format!("${{{{ runner.temp }}}}/{RUN_LOG_FILE}")),
            ),
            (
                "if-no-files-found".to_owned(),
                Value::Text("ignore".to_owned()),
            ),
            (
                "retention-days".to_owned(),
                Value::Int(CARGO_LOG_RETENTION_DAYS),
            ),
            ("overwrite".to_owned(), Value::Bool(true)),
        ],
        ..RenderedStep::default()
    }
}

pub(crate) fn upload_step(name: &str, file: &RepoPath) -> RenderedStep {
    RenderedStep {
        name: "Upload the tests".to_owned(),
        uses: Some(UPLOAD_ARTIFACT.to_owned()),
        with: vec![
            ("name".to_owned(), Value::Text(name.to_owned())),
            ("path".to_owned(), Value::Text(file.to_string())),
            ("compression-level".to_owned(), Value::Int(0)),
            (
                "retention-days".to_owned(),
                Value::Int(ARCHIVE_RETENTION_DAYS),
            ),
            (
                "if-no-files-found".to_owned(),
                Value::Text("error".to_owned()),
            ),
            ("overwrite".to_owned(), Value::Bool(true)),
        ],
        ..RenderedStep::default()
    }
}

pub(crate) fn download_step(name: &str, into: Option<&RepoPath>) -> RenderedStep {
    let mut with = vec![("name".to_owned(), Value::Text(name.to_owned()))];
    if let Some(into) = into {
        with.push(("path".to_owned(), Value::Text(into.to_string())));
    }
    RenderedStep {
        name: "Download the tests".to_owned(),
        uses: Some(DOWNLOAD_ARTIFACT.to_owned()),
        with,
        ..RenderedStep::default()
    }
}

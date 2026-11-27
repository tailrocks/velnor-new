// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use jiff::{Span, Timestamp};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::inputs::{NoFiles, UploadInputs, parse_int};
use super::js_text;
use super::transfer::put_blob;
use super::twirp::{Client, TwirpFailure};
use super::tz;
use super::{Blocking, client, escape, failed, search, validate_name, validate_path, zip};
use crate::exec::{ExecError, StepContext, StepResult};

fn expiry(ctx: &mut StepContext<'_>, days: Option<i64>) -> Result<Option<String>, ExecError> {
    let Some(mut days) = days else {
        return Ok(None);
    };
    let cap = ctx
        .env
        .get("GITHUB_RETENTION_DAYS")
        .and_then(|value| parse_int(js_text::trim_start(value)));
    if let Some(cap) = cap.filter(|cap| *cap != 0 && *cap < days) {
        ctx.log(&format!(
            "::warning::Retention days cannot be greater than the maximum allowed retention set within the repository. Using {cap} instead."
        ));
        days = cap;
    }
    let zone = tz::zone(ctx.env.get("TZ").map(String::as_str));
    let out_of_range = || ExecError::Invalid(format!("retention-days is out of range: {days}"));
    let at = Timestamp::now()
        .to_zoned_datetime(zone)
        .checked_add(Span::new().try_days(days).map_err(|_| out_of_range())?)
        .map_err(|_| out_of_range())?;
    OffsetDateTime::from_unix_timestamp_nanos(at.timestamp().as_nanosecond())
        .map_err(|_| out_of_range())?
        .format(&Rfc3339)
        .map(Some)
        .map_err(|error| failed(&error))
}

async fn replace_existing(client: &Client, name: &str) {
    let Ok(listed) = client.list(Some(name)).await else {
        return;
    };
    let newest = listed
        .into_iter()
        .filter(|artifact| artifact.name == name)
        .max_by_key(|artifact| artifact.id);
    if let Some(artifact) = newest {
        let _ = client.delete(&artifact).await;
    }
}

fn entries(
    root: &Path,
    files: Vec<(PathBuf, PathBuf)>,
) -> Result<Vec<(PathBuf, String)>, ExecError> {
    files
        .into_iter()
        .map(|(source, named)| {
            let relative = named
                .strip_prefix(root)
                .map_err(|_| {
                    failed(&format!(
                        "{} is outside {}",
                        named.display(),
                        root.display()
                    ))
                })?
                .components()
                .map(|part| {
                    part.as_os_str().to_str().map(str::to_owned).ok_or_else(|| {
                        failed(&format!(
                            "{} is not valid UTF-8, so it cannot be named in the archive",
                            named.display()
                        ))
                    })
                })
                .collect::<Result<Vec<_>, _>>()?
                .join("/");
            validate_path(&format!("/{relative}"))?;
            Ok((source, relative))
        })
        .collect()
}

fn artifact_url(ctx: &StepContext<'_>, id: u64) -> Option<String> {
    let env = |name: &str| ctx.env.get(name).filter(|value| !value.is_empty());
    Some(format!(
        "{}/{}/actions/runs/{}/artifacts/{id}",
        env("GITHUB_SERVER_URL")?,
        env("GITHUB_REPOSITORY")?,
        env("GITHUB_RUN_ID")?
    ))
}

fn report_no_files(
    ctx: &mut StepContext<'_>,
    inputs: &UploadInputs,
    outcome: StepResult,
) -> StepResult {
    let text = format!(
        "No files were found with the provided path: {}. No artifacts will be uploaded.",
        inputs.path
    );
    match inputs.if_no_files_found {
        NoFiles::Warn => ctx.log(&format!("::warning::{}", escape(&text))),
        NoFiles::Error => {
            ctx.log(&format!("::error::{}", escape(&text)));
            return StepResult::Failed;
        }
        NoFiles::Ignore => ctx.log(&text),
        NoFiles::Unrecognized => {}
    }
    outcome
}

pub(super) async fn run(
    ctx: &mut StepContext<'_>,
    blocking: &Blocking,
) -> Result<StepResult, ExecError> {
    let inputs = UploadInputs::parse(&ctx.inputs)?;
    for message in &inputs.failures {
        ctx.log(&format!("::error::{}", escape(message)));
    }
    let outcome = if inputs.failures.is_empty() {
        StepResult::Succeeded
    } else {
        StepResult::Failed
    };
    let home = ctx.env.get("HOME").map(PathBuf::from);
    let (workspace, text) = (ctx.workspace.to_path_buf(), inputs.path.clone());
    let found = blocking
        .run(move |stop| search::find(&workspace, home.as_deref(), &text, stop))
        .await??;
    if found.files.is_empty() {
        return Ok(report_no_files(ctx, &inputs, outcome));
    }
    let count = found.files.len();
    let plural = if count == 1 { "" } else { "s" };
    ctx.log(&format!(
        "With the provided path, there will be {count} file{plural} uploaded"
    ));
    let client = client(ctx);
    if inputs.overwrite
        && let Ok(client) = &client
    {
        replace_existing(client, &inputs.name).await;
        client.drain_notes(ctx);
    }
    validate_name(&inputs.name)?;
    let entries = entries(&found.root, found.files)?;
    let client = client?;
    let expires_at = expiry(ctx, inputs.retention_days)?;
    let temp = zip::TempZip::new(ctx.temp);
    let destination = temp.path().to_path_buf();
    let archive = blocking
        .run(move |stop| zip::write(&destination, &entries, stop))
        .await?
        .map_err(|error| failed(&error))?;
    let created = client.create(&inputs.name, expires_at.as_deref()).await;
    client.drain_notes(ctx);
    let upload_url = match created {
        Ok(url) => url,
        Err(error) if TwirpFailure::is_already_exists(&error) => {
            return Err(ExecError::Invalid(format!(
                "an artifact named '{}' already exists",
                inputs.name
            )));
        }
        Err(error) => return Err(failed(&error)),
    };
    put_blob(&upload_url, temp.path(), archive.size)
        .await
        .map_err(|error| failed(&error))?;
    let finalized = client
        .finalize(&inputs.name, archive.size, &archive.sha256)
        .await;
    client.drain_notes(ctx);
    let id = finalized.map_err(|error| failed(&error))?;
    ctx.log(&format!(
        "Artifact {} has been successfully uploaded! Final size is {} bytes. Artifact ID is {id}",
        inputs.name, archive.size
    ));
    ctx.set_output("artifact-id", id.to_string());
    ctx.set_output("artifact-digest", archive.sha256);
    if let Some(url) = artifact_url(ctx, id) {
        ctx.log(&format!("Artifact download URL: {url}"));
        ctx.set_output("artifact-url", url);
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    use std::path::PathBuf;

    use super::entries;

    #[test]
    fn a_name_that_is_not_utf8_is_refused_instead_of_renamed() {
        let root = PathBuf::from("/work");
        let first = PathBuf::from(OsStr::from_bytes(b"/work/a\x80"));
        let second = PathBuf::from(OsStr::from_bytes(b"/work/a\x81"));
        let refused = entries(
            &root,
            vec![(first.clone(), first), (second.clone(), second)],
        );
        assert!(refused.is_err());
        let fine = PathBuf::from("/work/a.txt");
        let kept = entries(&root, vec![(fine.clone(), fine)]).ok();
        assert_eq!(
            kept.map(|names| names[0].1.clone()),
            Some("a.txt".to_owned())
        );
    }
}

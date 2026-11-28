// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;

use anyhow::{Context, Result};
use jiff::tz::TimeZone;
use jiff::{Span, Timestamp};
use serde_json::json;

use super::artifact_support::{
    Env, JOB_RUN, OLD_JOB, output, run_job, run_job_env, write_file, zip_of,
};
use super::exec_support::{Ran, Scratch};
use super::fake_results::FakeResults;
use super::native_artifact_texts::name_error;
use super::native_artifact_upstream::days_that_cross_a_clock_change;
use super::support::{Reply, TestServer};

const OLDER_LONGER: &[u8] = b"old and longer than the new bytes";
const BLOB_RETRY: &[(&str, &str)] = &[("VELNOR_ARTIFACT_BLOB_BASE_MS", "20")];

fn upload_inputs<'a>(path: &'a str, extra: &[(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
    let mut inputs = vec![("name", "bundle"), ("path", path)];
    inputs.extend_from_slice(extra);
    inputs
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_path_emptied_by_negation_markers_is_refused_like_the_toolkit_refuses_it() -> Result<()> {
    for path in ["!!", "!", "a.txt\n! !", "!!!!"] {
        let fake = FakeResults::start().await?;
        let scratch = Scratch::new()?;
        write_file(&scratch, "a.txt", b"a")?;
        let ran = run_job(
            &scratch,
            "artifact_upload",
            &fake.url(),
            &upload_inputs(path, &[]),
        )?;
        assert_eq!(ran.code, Some(1), "{path:?}: {}", ran.stdout);
        assert!(
            ran.has_line("::error::pattern cannot be empty"),
            "{path:?}: {}",
            ran.stdout
        );
        assert_eq!(fake.calls("CreateArtifact")?.len(), 0, "{path:?}");
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unrecognized_no_files_policy_fails_the_step_after_the_toolkit_uploads_anyway()
-> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    write_file(&scratch, "a.txt", b"a")?;
    let ran = run_job(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &upload_inputs("a.txt", &[("if-no-files-found", "bogus")]),
    )?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert!(
        ran.has_line("::error::Unrecognized if-no-files-found input. Provided: bogus. Available options: warn,error,ignore"),
        "{}",
        ran.stdout
    );
    assert_eq!(fake.artifacts()?.len(), 1, "{}", ran.stdout);
    assert!(output(&ran, "artifact-id").is_some(), "{}", ran.stdout);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unrecognized_no_files_policy_says_nothing_more_when_nothing_matches() -> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    let ran = run_job(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &upload_inputs("missing.txt", &[("if-no-files-found", "bogus")]),
    )?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert!(
        !ran.stdout.contains("No files were found"),
        "{}",
        ran.stdout
    );
    assert_eq!(fake.calls("CreateArtifact")?.len(), 0);
    Ok(())
}

fn position(ran: &Ran, prefix: &str) -> Result<usize> {
    ran.stdout
        .lines()
        .position(|line| line.starts_with(prefix))
        .with_context(|| format!("no line starts with {prefix}: {}", ran.stdout))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unsafe_name_is_validated_after_the_log_line_and_the_overwrite_delete() -> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    write_file(&scratch, "a.txt", b"a")?;
    let ran = run_job(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &[("name", "a:b"), ("path", "a.txt"), ("overwrite", "true")],
    )?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    let logged = position(
        &ran,
        "With the provided path, there will be 1 file uploaded",
    )?;
    let refused = position(&ran, &name_error("a:b", "Colon :"))?;
    assert!(logged < refused, "{}", ran.stdout);
    assert_eq!(fake.calls("ListArtifacts")?.len(), 1, "{}", ran.stdout);
    assert_eq!(fake.calls("CreateArtifact")?.len(), 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_expiry_zone_follows_the_toolkit_rules_for_the_step_tz() -> Result<()> {
    let zone = TimeZone::get("America/Los_Angeles")?;
    let days = days_that_cross_a_clock_change(&zone)?;
    let now = Timestamp::now();
    let calendar = now
        .to_zoned_datetime(zone)
        .checked_add(Span::new().try_days(days)?)?
        .timestamp();
    let fixed = now.checked_add(Span::new().try_hours(days * 24)?)?;
    let rows = [
        ("UTC", "America/Los_Angeles", calendar),
        ("UTC", ":America/Los_Angeles", calendar),
        ("America/Los_Angeles", "", fixed),
        (
            "America/Los_Angeles",
            "/usr/share/zoneinfo/America/Los_Angeles",
            fixed,
        ),
        ("America/Los_Angeles", "Bogus/Zone", fixed),
    ];
    for (process, step, expected) in rows {
        let fake = FakeResults::start().await?;
        let scratch = Scratch::new()?;
        write_file(&scratch, "a.txt", b"a")?;
        let retention = days.to_string();
        let ran = run_job_env(
            &scratch,
            "artifact_upload",
            &fake.url(),
            &upload_inputs("a.txt", &[("retention-days", &retention)]),
            Env {
                process: &[("TZ", process)],
                job: &[("TZ", step)],
                ..Env::default()
            },
        )?;
        assert_eq!(ran.code, Some(0), "{step:?}: {}", ran.stdout);
        let sent = fake.calls("CreateArtifact")?[0]["expires_at"]
            .as_str()
            .context("CreateArtifact sent no expires_at")?
            .parse::<Timestamp>()?;
        let drift = (sent.as_second() - expected.as_second()).abs();
        assert!(drift < 120, "{step:?}: {sent} against {expected}");
    }
    Ok(())
}

async fn download_over(existing_mode: u32) -> Result<(Scratch, Ran)> {
    let fake = FakeResults::start().await?;
    fake.seed("a", JOB_RUN, OLD_JOB, zip_of(&[("m.json", b"new")])?)?;
    let scratch = Scratch::new()?;
    write_file(&scratch, "out/m.json", OLDER_LONGER)?;
    let existing = scratch.workspace().join("out/m.json");
    std::fs::hard_link(&existing, scratch.workspace().join("out/twin.json"))?;
    std::fs::set_permissions(&existing, std::fs::Permissions::from_mode(existing_mode))?;
    let ran = run_job(
        &scratch,
        "artifact_download",
        &fake.url(),
        &[("pattern", "a"), ("path", "out")],
    )?;
    Ok((scratch, ran))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_download_writes_through_the_existing_file_as_the_toolkit_stream_does() -> Result<()> {
    let (scratch, ran) = download_over(0o644).await?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert_eq!(
        std::fs::read(scratch.workspace().join("out/twin.json"))?,
        b"new"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_download_cannot_replace_a_file_the_toolkit_could_not_open_for_writing() -> Result<()> {
    if rustix::process::geteuid().is_root() {
        return Ok(());
    }
    let (scratch, ran) = download_over(0o444).await?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert!(ran.stdout.contains("Permission denied"), "{}", ran.stdout);
    assert_eq!(
        std::fs::read(scratch.workspace().join("out/m.json"))?,
        OLDER_LONGER
    );
    Ok(())
}

struct Redirected {
    fake: FakeResults,
    hop: TestServer,
    scratch: Scratch,
}

async fn redirected(
    first: u16,
    then: impl Fn(&str, &str) -> Reply + Send + Sync + 'static,
) -> Result<Redirected> {
    let fake = FakeResults::start().await?;
    let id = fake.seed("a", JOB_RUN, OLD_JOB, zip_of(&[("m.json", b"moved")])?)?;
    let hop = TestServer::start().await?;
    let blob = format!("{}/blob?id={id}", fake.url());
    let second = format!("{}/second", hop.uri());
    fake.script(
        "GetSignedArtifactURL",
        vec![Reply::json(
            200,
            &json!({ "signed_url": format!("{}/moved", hop.uri()) }),
        )],
    )?;
    let (to_second, to_blob) = (second.clone(), blob.clone());
    hop.respond("GET", "/moved", move |_| {
        Reply::status(first).with_header("location", &to_second)
    })?;
    hop.respond("GET", "/second", move |_| then(&second, &to_blob))?;
    Ok(Redirected {
        fake,
        hop,
        scratch: Scratch::new()?,
    })
}

impl Redirected {
    fn download(&self) -> Result<Ran> {
        run_job_env(
            &self.scratch,
            "artifact_download",
            &self.fake.url(),
            &[("path", "restored")],
            Env {
                process: BLOB_RETRY,
                ..Env::default()
            },
        )
    }

    fn hops(&self, path: &str) -> Result<usize> {
        Ok(self.hop.requests_to("GET", path)?.len())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_blob_download_follows_the_redirects_the_toolkit_client_follows() -> Result<()> {
    for status in [301_u16, 302, 303, 307, 308] {
        let moved = redirected(status, |_, blob| {
            Reply::status(307).with_header("location", blob)
        })
        .await?;
        let ran = moved.download()?;
        assert_eq!(ran.code, Some(0), "{status}: {}", ran.stdout);
        assert_eq!(
            std::fs::read(moved.scratch.workspace().join("restored/m.json"))?,
            b"moved",
            "{status}"
        );
        assert_eq!(moved.fake.blob_gets()?, 1, "{status}");
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_redirect_status_outside_the_toolkit_list_is_not_followed() -> Result<()> {
    let moved = redirected(300, |_, blob| {
        Reply::status(307).with_header("location", blob)
    })
    .await?;
    let ran = moved.download()?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert!(ran.stdout.contains("status=300"), "{}", ran.stdout);
    assert_eq!(moved.hops("/moved")?, 1);
    assert_eq!(moved.fake.blob_gets()?, 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_blob_download_gives_up_after_the_toolkit_limit_of_fifty_redirects() -> Result<()> {
    let moved = redirected(302, |second, _| {
        Reply::status(302).with_header("location", second)
    })
    .await?;
    let ran = moved.download()?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert!(ran.stdout.contains("status=302"), "{}", ran.stdout);
    assert_eq!(moved.hops("/moved")? + moved.hops("/second")?, 51);
    assert_eq!(moved.fake.blob_gets()?, 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_relative_redirect_location_is_retried_like_the_toolkit_url_error() -> Result<()> {
    let moved = redirected(302, |_, _| {
        Reply::status(302).with_header("location", "/relative")
    })
    .await?;
    let ran = moved.download()?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert!(
        ran.stdout
            .contains("Artifact download failed after 5 retries."),
        "{}",
        ran.stdout
    );
    assert_eq!(moved.hops("/moved")?, 5);
    assert_eq!(moved.hops("/second")?, 5);
    assert_eq!(moved.fake.blob_gets()?, 0);
    Ok(())
}

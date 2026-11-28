// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use anyhow::{Context, Result};
use jiff::tz::TimeZone;
use jiff::{Span, Timestamp};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::artifact_support::{
    Env, JOB_RUN, OLD_JOB, entries_of, run_job, run_job_env, write_file, zip_of,
};
use super::exec_support::Scratch;
use super::fake_results::FakeResults;

async fn uploaded(files: &[(&str, &[u8])], path: &str) -> Result<Vec<String>> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    for (name, bytes) in files {
        write_file(&scratch, name, bytes)?;
    }
    let ran = run_job(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &[("name", "bundle"), ("path", path)],
    )?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    Ok(entries_of(&fake.artifacts()?[0].zip)?
        .into_iter()
        .map(|(name, _)| name)
        .collect())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_upload_line_is_trimmed_like_the_toolkit_trims_it() -> Result<()> {
    let names = uploaded(&[("a.txt", b"a"), ("\u{85}a.txt", b"n")], "\u{85}a.txt").await?;
    assert_eq!(names, ["\u{85}a.txt"]);
    let names = uploaded(&[("a.txt", b"a"), ("b.txt", b"b")], "a.txt\u{feff}\nb.txt").await?;
    assert_eq!(names, ["a.txt", "b.txt"]);
    let names = uploaded(&[(" a.txt", b"s"), ("a.txt", b"a")], "./ a.txt").await?;
    assert_eq!(names, [" a.txt"]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_download_path_keeps_a_leading_next_line() -> Result<()> {
    let fake = FakeResults::start().await?;
    fake.seed("a", JOB_RUN, OLD_JOB, zip_of(&[("m.json", b"one")])?)?;
    let scratch = Scratch::new()?;
    let ran = run_job(
        &scratch,
        "artifact_download",
        &fake.url(),
        &[("pattern", "a"), ("path", "\u{85}out")],
    )?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert!(
        scratch.workspace().join("\u{85}out/m.json").exists(),
        "{}",
        ran.stdout
    );
    Ok(())
}

async fn upload_with_cap(cap: &str) -> Result<(FakeResults, super::exec_support::Ran)> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    write_file(&scratch, "a.txt", b"a")?;
    let ran = run_job_env(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &[
            ("name", "bundle"),
            ("path", "a.txt"),
            ("retention-days", "90"),
        ],
        Env {
            job: &[("GITHUB_RETENTION_DAYS", cap)],
            ..Env::default()
        },
    )?;
    assert_eq!(ran.code, Some(0), "{cap}: {}", ran.stdout);
    Ok((fake, ran))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_repository_retention_cap_is_parsed_like_parse_int() -> Result<()> {
    for (cap, days) in [("30 days", 30), ("\u{feff} 30", 30), ("\u{85}30", 90)] {
        let (fake, _) = upload_with_cap(cap).await?;
        super::artifact_support::expires_in(&fake.calls("CreateArtifact")?[0], days)
            .with_context(|| format!("cap {cap:?}"))?;
    }
    Ok(())
}

pub(super) fn days_that_cross_a_clock_change(zone: &TimeZone) -> Result<i64> {
    let now = Timestamp::now().to_zoned_datetime(zone.clone());
    let shifted = |days: i64| -> Result<bool> {
        Ok(now.checked_add(Span::new().try_days(days)?)?.offset() != now.offset())
    };
    for days in 30..400 {
        if shifted(days - 1)? && shifted(days)? && shifted(days + 1)? {
            return Ok(days);
        }
    }
    anyhow::bail!("no clock change within 400 days")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_expiry_adds_days_on_the_local_calendar() -> Result<()> {
    let zone = TimeZone::get("America/Los_Angeles")?;
    let days = days_that_cross_a_clock_change(&zone)?;
    let expected = Timestamp::now()
        .to_zoned_datetime(zone)
        .checked_add(Span::new().try_days(days)?)?
        .timestamp();
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    write_file(&scratch, "a.txt", b"a")?;
    let ran = run_job_env(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &[
            ("name", "bundle"),
            ("path", "a.txt"),
            ("retention-days", &days.to_string()),
        ],
        Env {
            process: &[("TZ", "America/Los_Angeles")],
            ..Env::default()
        },
    )?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    let sent = fake.calls("CreateArtifact")?[0]["expires_at"]
        .as_str()
        .context("CreateArtifact sent no expires_at")?
        .parse::<Timestamp>()?;
    let drift = (sent.as_second() - expected.as_second()).abs();
    assert!(drift < 120, "{sent} against {expected}");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_name_the_toolkit_never_validates_does_not_stop_an_upload_that_finds_no_files()
-> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    let ran = run_job(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &[("name", "a:b"), ("path", "nothing-here")],
    )?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert!(
        ran.stdout
            .contains("::warning::No files were found with the provided path: nothing-here."),
        "{}",
        ran.stdout
    );
    assert_eq!(fake.calls("CreateArtifact")?.len(), 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_download_over_an_existing_file_keeps_its_mode() -> Result<()> {
    let fake = FakeResults::start().await?;
    fake.seed("a", JOB_RUN, OLD_JOB, zip_of(&[("m.json", b"new")])?)?;
    let scratch = Scratch::new()?;
    write_file(&scratch, "out/m.json", b"old")?;
    let existing = scratch.workspace().join("out/m.json");
    std::fs::set_permissions(&existing, std::fs::Permissions::from_mode(0o751))?;
    let ran = run_job(
        &scratch,
        "artifact_download",
        &fake.url(),
        &[("pattern", "a"), ("path", "out")],
    )?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert_eq!(std::fs::read(&existing)?, b"new");
    assert_eq!(
        std::fs::metadata(&existing)?.permissions().mode() & 0o777,
        0o751
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_service_call_is_cut_by_socket_silence_not_by_its_duration() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let server = tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut request = [0_u8; 4096];
                let _ = stream.read(&mut request).await;
                let head = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 16\r\n\r\n";
                let _ = stream.write_all(head.as_bytes()).await;
                for piece in ["{\"artifacts\":", "[", "]}"] {
                    let _ = stream.write_all(piece.as_bytes()).await;
                    let _ = stream.flush().await;
                    tokio::time::sleep(Duration::from_millis(2000)).await;
                }
            });
        }
    });
    let scratch = Scratch::new()?;
    let ran = run_job_env(
        &scratch,
        "artifact_download",
        &format!("http://127.0.0.1:{port}"),
        &[],
        Env {
            process: &[
                ("VELNOR_ARTIFACT_CALL_TIMEOUT_MS", "3000"),
                ("VELNOR_ARTIFACT_RPC_BASE_MS", "20"),
            ],
            ..Env::default()
        },
    )?;
    server.abort();
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert!(
        ran.has_line("Total of 0 artifact(s) downloaded"),
        "{}",
        ran.stdout
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_marker_is_followed_by_the_toolkit_trim_not_the_rust_trim() -> Result<()> {
    let names = uploaded(
        &[("a.txt", b"a"), ("\u{85}a.txt", b"n"), ("b.txt", b"b")],
        "a.txt\n\u{85}a.txt\nb.txt\n!\u{85}a.txt\n!\u{feff}b.txt",
    )
    .await?;
    assert_eq!(names, ["a.txt"]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_service_call_that_is_silent_for_thirty_five_seconds_still_succeeds() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let server = tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut request = [0_u8; 4096];
                let _ = stream.read(&mut request).await;
                tokio::time::sleep(Duration::from_secs(35)).await;
                let body = "{\"artifacts\":[]}";
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes()).await;
            });
        }
    });
    let scratch = Scratch::new()?;
    let ran = run_job(
        &scratch,
        "artifact_download",
        &format!("http://127.0.0.1:{port}"),
        &[],
    )?;
    server.abort();
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert!(
        ran.has_line("Total of 0 artifact(s) downloaded"),
        "{}",
        ran.stdout
    );
    Ok(())
}

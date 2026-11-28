// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::Result;
use serde_json::json;

use super::artifact_support::{
    Env, JOB_RUN, OLD_JOB, error_lines, read_file, run_job, run_job_env, spawn_job, write_file,
    zip_of,
};
use super::exec_support::Scratch;
use super::fake_results::FakeResults;
use super::support::Reply;

const ATTEMPTS: usize = 5;
const STALL_ENV: &[(&str, &str)] = &[
    ("VELNOR_ARTIFACT_CALL_TIMEOUT_MS", "300"),
    ("VELNOR_ARTIFACT_BLOB_BASE_MS", "20"),
];

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_download_that_stalls_midway_fails_by_the_idle_timeout() -> Result<()> {
    let fake = FakeResults::start().await?;
    fake.seed(
        "ci-metrics-a",
        JOB_RUN,
        OLD_JOB,
        zip_of(&[("m.json", &[b'x'; 4096])])?,
    )?;
    fake.stall_downloads_halfway()?;
    let scratch = Scratch::new()?;
    let finished = spawn_job(&scratch, "artifact_download", &fake.url(), &[], STALL_ENV)?
        .finish("the runner to give up on the stalled download")?;
    assert_eq!(finished.code, Some(1), "{}", finished.stdout);
    assert_eq!(fake.blob_gets()?, ATTEMPTS, "{}", finished.stdout);
    assert_eq!(
        error_lines(&finished.stdout),
        [
            "::error::Unable to download artifact(s): Unable to download and extract artifact: Artifact download failed after 5 retries."
        ]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_retention_beyond_the_date_range_fails_the_step_without_a_panic() -> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    write_file(&scratch, "a.txt", b"a")?;
    let ran = run_job(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &[
            ("name", "bundle"),
            ("path", "a.txt"),
            ("retention-days", "4294967295"),
        ],
    )?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert!(
        ran.has_line("::error::retention-days is out of range: 4294967295"),
        "{}",
        ran.stdout
    );
    assert_eq!(fake.calls("CreateArtifact")?.len(), 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unsupported_pattern_stops_the_job_before_the_first_step_runs() -> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    let ran = run_job_env(
        &scratch,
        "artifact_download",
        &fake.url(),
        &[("pattern", "@(ci-a|ci-b)")],
        Env {
            script_before: Some("echo ran > before.txt"),
            ..Env::default()
        },
    )?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert!(
        ran.stdout
            .contains("unsupported: actions/download-artifact pattern '@(ci-a|ci-b)'"),
        "{}",
        ran.stdout
    );
    assert!(!scratch.workspace().join("before.txt").exists());
    assert_eq!(fake.calls("ListArtifacts")?.len(), 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_non_zip_artifact_is_written_under_its_advertised_name() -> Result<()> {
    let fake = FakeResults::start().await?;
    fake.seed_raw(
        "ci-metrics-raw",
        JOB_RUN,
        OLD_JOB,
        b"raw bytes".to_vec(),
        "attachment; filename=\"metrics.json\"",
    )?;
    let scratch = Scratch::new()?;
    let ran = run_job(&scratch, "artifact_download", &fake.url(), &[])?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert_eq!(
        read_file(scratch.workspace().join("restored/metrics.json"))?,
        b"raw bytes"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_non_zip_artifact_name_cannot_leave_the_destination() -> Result<()> {
    let fake = FakeResults::start().await?;
    fake.seed_raw(
        "ci-metrics-raw",
        JOB_RUN,
        OLD_JOB,
        b"raw bytes".to_vec(),
        "attachment; filename*=UTF-8''..%2F..%2Fna%C3%AFve.txt",
    )?;
    let scratch = Scratch::new()?;
    let ran = run_job(&scratch, "artifact_download", &fake.url(), &[])?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert_eq!(
        read_file(scratch.workspace().join("restored/na\u{ef}ve.txt"))?,
        b"raw bytes"
    );
    assert!(!scratch.workspace().join("na\u{ef}ve.txt").exists());
    let parent = scratch.workspace().with_file_name("na\u{ef}ve.txt");
    assert!(!parent.exists(), "{}", parent.display());
    Ok(())
}

const UPLOAD_LIMIT: &str = "1048576";

fn upload_with_limit(scratch: &Scratch, url: &str) -> Result<super::exec_support::Ran> {
    run_job_env(
        scratch,
        "artifact_upload",
        url,
        &[("name", "bundle"), ("path", "big.bin")],
        Env {
            process: &[("VELNOR_ARTIFACT_UPLOAD_LIMIT", UPLOAD_LIMIT)],
            ..Env::default()
        },
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_upload_limit_applies_to_the_archive_not_to_the_source_bytes() -> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    write_file(&scratch, "big.bin", &vec![0_u8; 3 << 20])?;
    let ran = upload_with_limit(&scratch, &fake.url())?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert!(fake.artifacts()?[0].zip.len() < 1 << 20);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_archive_over_the_upload_limit_is_refused_before_any_blob_is_sent() -> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    let noise: Vec<u8> = (0..2 << 20)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state.to_le_bytes()[0]
        })
        .collect();
    write_file(&scratch, "big.bin", &noise)?;
    let ran = upload_with_limit(&scratch, &fake.url())?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert!(
        ran.has_line(
            "::error::artifact is larger than the 1048576-byte upload limit; split it into smaller uploads"
        ),
        "{}",
        ran.stdout
    );
    assert_eq!(fake.artifacts()?.len(), 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_upload_that_stalls_midway_fails_by_the_idle_timeout() -> Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let accepted = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&accepted);
    let holder = tokio::spawn(async move {
        let mut open = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            count.fetch_add(1, Ordering::SeqCst);
            open.push(stream);
        }
    });
    let fake = FakeResults::start().await?;
    fake.script(
        "CreateArtifact",
        vec![Reply::json(
            200,
            &json!({
                "ok": true,
                "signed_upload_url": format!("http://127.0.0.1:{port}/blob?sig=x"),
            }),
        )],
    )?;
    let scratch = Scratch::new()?;
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    let noise: Vec<u8> = (0..24 << 20)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state.to_le_bytes()[0]
        })
        .collect();
    write_file(&scratch, "big.bin", &noise)?;
    let finished = spawn_job(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &[("name", "bundle"), ("path", "big.bin")],
        STALL_ENV,
    )?
    .finish("the runner to give up on the stalled upload");
    holder.abort();
    let finished = finished?;
    assert_eq!(finished.code, Some(1), "{}", finished.stdout);
    assert_eq!(
        accepted.load(Ordering::SeqCst),
        ATTEMPTS,
        "{}",
        finished.stdout
    );
    assert_eq!(
        error_lines(&finished.stdout),
        ["::error::artifact blob PUT: request timed out"]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failing_blob_upload_does_not_print_the_signed_url_query() -> Result<()> {
    let fake = FakeResults::start().await?;
    let denied = format!("{}/denied?sig=topsecret", fake.url());
    fake.script(
        "CreateArtifact",
        vec![Reply::json(
            200,
            &json!({ "ok": true, "signed_upload_url": denied }),
        )],
    )?;
    let scratch = Scratch::new()?;
    write_file(&scratch, "a.txt", b"a")?;
    let ran = run_job(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &[("name", "bundle"), ("path", "a.txt")],
    )?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert_eq!(
        error_lines(&ran.stdout),
        ["::error::artifact blob PUT failed: status=404 Not Found"],
        "{}",
        ran.stdout
    );
    assert!(
        !ran.stdout.contains("topsecret") && !ran.stderr.contains("topsecret"),
        "{}{}",
        ran.stdout,
        ran.stderr
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_blob_upload_to_a_closed_port_does_not_print_the_signed_url_query() -> Result<()> {
    let closed = crate::support::ClosedPort::start()?;
    let port = closed.port();
    let fake = FakeResults::start().await?;
    fake.script(
        "CreateArtifact",
        vec![Reply::json(
            200,
            &json!({
                "ok": true,
                "signed_upload_url": format!("http://127.0.0.1:{port}/blob?sig=topsecret"),
            }),
        )],
    )?;
    let scratch = Scratch::new()?;
    write_file(&scratch, "a.txt", b"a")?;
    let ran = run_job_env(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &[("name", "bundle"), ("path", "a.txt")],
        Env {
            process: &[("VELNOR_ARTIFACT_BLOB_BASE_MS", "20")],
            ..Env::default()
        },
    )?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert_eq!(
        error_lines(&ran.stdout),
        ["::error::artifact blob PUT: request transport failed"],
        "{}",
        ran.stdout
    );
    assert!(
        !ran.stdout.contains("topsecret") && !ran.stderr.contains("topsecret"),
        "{}{}",
        ran.stdout,
        ran.stderr
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_blob_answered_with_any_status_but_200_fails_without_a_retry() -> Result<()> {
    for status in [204_u16, 206] {
        let fake = FakeResults::start().await?;
        fake.seed(
            "ci-metrics-a",
            JOB_RUN,
            OLD_JOB,
            zip_of(&[("m.json", b"partial")])?,
        )?;
        fake.answer_blobs_with(status)?;
        let scratch = Scratch::new()?;
        let ran = run_job(
            &scratch,
            "artifact_download",
            &fake.url(),
            &[("path", "restored")],
        )?;
        assert_eq!(ran.code, Some(1), "{status}: {}", ran.stdout);
        assert!(
            ran.stdout
                .contains(&format!("artifact blob GET failed: status={status}")),
            "{status}: {}",
            ran.stdout
        );
        assert_eq!(fake.blob_gets()?, 1, "{status}");
        assert!(!scratch.workspace().join("restored/m.json").exists());
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upload_accepts_the_dot_names_the_pinned_toolkit_accepts() -> Result<()> {
    for name in [".", ".."] {
        let fake = FakeResults::start().await?;
        let scratch = Scratch::new()?;
        write_file(&scratch, "a.txt", b"a")?;
        let ran = run_job(
            &scratch,
            "artifact_upload",
            &fake.url(),
            &[("name", name), ("path", "a.txt")],
        )?;
        assert_eq!(ran.code, Some(0), "{name}: {}", ran.stdout);
        assert_eq!(fake.calls("CreateArtifact")?.len(), 1, "{name}");
    }
    Ok(())
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Write as _;

use anyhow::Result;

use super::artifact_support::{
    Env, JOB_RUN, OLD_JOB, error_lines, read_file, run_job, run_job_env, spawn_job, wait_until,
    write_file, zip_of,
};
use super::exec_support::Scratch;
use super::fake_results::FakeResults;

const EXTRACTION_LIMIT: u64 = 5 << 30;
const LIMIT_LINE: &str = "::error::artifact holds more than 5368709120 bytes of files, which download-artifact refuses to extract; split it into artifacts of at most that size";

fn sparse(scratch: &Scratch, relative: &str, length: u64) -> Result<()> {
    write_file(scratch, relative, b"")?;
    std::fs::OpenOptions::new()
        .write(true)
        .open(scratch.workspace().join(relative))?
        .set_len(length)?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_upload_over_the_extraction_bound_is_refused_before_any_work() -> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    sparse(&scratch, "big.bin", EXTRACTION_LIMIT + 1)?;
    let ran = run_job(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &[("name", "bundle"), ("path", "big.bin")],
    )?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert_eq!(error_lines(&ran.stdout), [LIMIT_LINE], "{}", ran.stdout);
    assert_eq!(fake.calls("CreateArtifact")?.len(), 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_extraction_bound_counts_the_bytes_of_every_file() -> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    let half = EXTRACTION_LIMIT / 2 + 1;
    sparse(&scratch, "out/one.bin", half)?;
    sparse(&scratch, "out/two.bin", half)?;
    let ran = run_job(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &[("name", "bundle"), ("path", "out")],
    )?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert_eq!(error_lines(&ran.stdout), [LIMIT_LINE], "{}", ran.stdout);
    assert_eq!(fake.calls("CreateArtifact")?.len(), 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_retention_beyond_the_integer_range_uses_the_repository_maximum() -> Result<()> {
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
            ("retention-days", "99999999999"),
        ],
        Env {
            job: &[("GITHUB_RETENTION_DAYS", "90")],
            ..Env::default()
        },
    )?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert!(
        ran.has_line("::warning::Retention days cannot be greater than the maximum allowed retention set within the repository. Using 90 instead."),
        "{}",
        ran.stdout
    );
    super::artifact_support::expires_in(&fake.calls("CreateArtifact")?[0], 90)
}

fn noise(length: usize, seed: u64) -> Vec<u8> {
    let mut state = seed;
    (0..length)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state.to_le_bytes()[0]
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_that_grows_while_it_is_archived_cannot_pass_the_extraction_bound() -> Result<()> {
    const BOUND: usize = 64 << 20;
    let appended = noise(8 << 20, 0x9e37_79b9_7f4a_7c15);
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    write_file(
        &scratch,
        "grow.bin",
        &noise(BOUND - (4 << 20), 0x2545_f491_4f6c_dd1d),
    )?;
    let running = spawn_job(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &[("name", "bundle"), ("path", "grow.bin")],
        &[("VELNOR_ARTIFACT_FILES_LIMIT", &BOUND.to_string())],
    )?;
    let temp = scratch.temp();
    wait_until("the archive to be created", || {
        std::fs::read_dir(&temp).is_ok_and(|entries| {
            entries.flatten().any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("velnor-artifact-")
            })
        })
    })?;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(scratch.workspace().join("grow.bin"))?;
    file.write_all(&appended)?;
    drop(file);
    let finished = running.finish("the runner to refuse the grown file")?;
    assert_eq!(finished.code, Some(1), "{}", finished.stdout);
    assert_eq!(
        error_lines(&finished.stdout),
        [format!(
            "::error::a file grew while it was being archived and the artifact now holds more than {BOUND} bytes of files, which download-artifact refuses to extract; archive the files once they stop changing"
        )],
        "{}",
        finished.stdout
    );
    assert_eq!(fake.calls("CreateArtifact")?.len(), 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_entry_that_may_grow_past_4_gib_is_written_with_zip64() -> Result<()> {
    let limit = u64::from(u32::MAX);
    for (bound, zip64) in [(limit - 2, false), (limit - 1, true), (limit, true)] {
        let fake = FakeResults::start().await?;
        let scratch = Scratch::new()?;
        write_file(&scratch, "a.txt", b"small")?;
        let running = spawn_job(
            &scratch,
            "artifact_upload",
            &fake.url(),
            &[("name", "bundle"), ("path", "a.txt")],
            &[("VELNOR_ARTIFACT_FILES_LIMIT", &bound.to_string())],
        )?;
        let ran = running.finish("the upload to finish")?;
        assert_eq!(ran.code, Some(0), "{bound}: {}", ran.stdout);
        let zip = fake.artifacts()?[0].zip.clone();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip))?;
        let member = archive.by_index(0)?;
        assert_eq!(
            member
                .extra_data()
                .is_some_and(|extra| extra.starts_with(&[1, 0, 16, 0])),
            zip64,
            "limit {bound}: {:?}",
            member.extra_data()
        );
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retention_days_are_parsed_like_parse_int() -> Result<()> {
    for (text, days) in [
        ("1.5", 1),
        ("7days", 7),
        ("0x10", 16),
        ("+3", 3),
        ("-3", -3),
    ] {
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
                ("retention-days", text),
            ],
        )?;
        assert_eq!(ran.code, Some(0), "{text}: {}", ran.stdout);
        super::artifact_support::expires_in(&fake.calls("CreateArtifact")?[0], days)?;
    }
    for text in ["days", "0x", ".5"] {
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
                ("retention-days", text),
            ],
        )?;
        assert_eq!(ran.code, Some(1), "{text}: {}", ran.stdout);
        assert!(
            ran.has_line("::error::Invalid retention-days"),
            "{text}: {}",
            ran.stdout
        );
        assert!(
            fake.calls("CreateArtifact")?[0]["expires_at"].is_null(),
            "{text}"
        );
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_single_artifact_named_with_dots_extracts_into_the_path() -> Result<()> {
    for name in [".", ".."] {
        let fake = FakeResults::start().await?;
        fake.seed(name, JOB_RUN, OLD_JOB, zip_of(&[("m.json", b"kept")])?)?;
        let scratch = Scratch::new()?;
        let ran = run_job(
            &scratch,
            "artifact_download",
            &fake.url(),
            &[("path", "restored")],
        )?;
        assert_eq!(ran.code, Some(0), "{name}: {}", ran.stdout);
        assert_eq!(
            read_file(scratch.workspace().join("restored/m.json"))?,
            b"kept",
            "{name}"
        );
    }
    Ok(())
}

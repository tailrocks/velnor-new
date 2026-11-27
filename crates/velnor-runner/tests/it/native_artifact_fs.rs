// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::File;
use std::os::unix::fs::symlink;
use std::time::{Duration, UNIX_EPOCH};

use zip::{DateTime, ZipArchive};

use anyhow::{Context, Result};

use super::artifact_support::{
    Env, JOB_RUN, OLD_JOB, entries_of, read_file, run_job, run_job_env, write_file, zip_of,
};
use super::exec_support::Scratch;
use super::fake_results::FakeResults;
use super::zip_forgery::forged_zip;

fn names(zip: &[u8]) -> Result<Vec<(String, Vec<u8>)>> {
    entries_of(zip)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upload_archives_a_file_symlink_under_the_link_path() -> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    write_file(&scratch, "out/v1.txt", b"one")?;
    symlink("v1.txt", scratch.workspace().join("out/latest.txt"))?;
    let ran = run_job(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &[("name", "bundle"), ("path", "out")],
    )?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert_eq!(
        names(&fake.artifacts()?[0].zip)?,
        [
            ("latest.txt".to_owned(), b"one".to_vec()),
            ("v1.txt".to_owned(), b"one".to_vec()),
        ]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upload_through_a_symlinked_parent_keeps_the_paths_relative_to_the_root() -> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    write_file(&scratch, "real/out/a.txt", b"one")?;
    symlink("real", scratch.workspace().join("link"))?;
    let ran = run_job(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &[("name", "bundle"), ("path", "link/out")],
    )?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert_eq!(
        names(&fake.artifacts()?[0].zip)?,
        [("a.txt".to_owned(), b"one".to_vec())]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upload_archives_a_symlink_whose_target_is_outside_the_root() -> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    write_file(&scratch, "store/real.tar", b"tar")?;
    std::fs::create_dir_all(scratch.workspace().join("dist"))?;
    symlink(
        scratch.workspace().join("store/real.tar"),
        scratch.workspace().join("dist/app.tar"),
    )?;
    let ran = run_job(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &[("name", "bundle"), ("path", "dist")],
    )?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert_eq!(
        names(&fake.artifacts()?[0].zip)?,
        [("app.tar".to_owned(), b"tar".to_vec())]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upload_of_a_single_symlink_keeps_its_name() -> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    write_file(&scratch, "v1.txt", b"one")?;
    symlink("v1.txt", scratch.workspace().join("latest.txt"))?;
    let ran = run_job(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &[("name", "bundle"), ("path", "latest.txt")],
    )?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert_eq!(
        names(&fake.artifacts()?[0].zip)?,
        [("latest.txt".to_owned(), b"one".to_vec())]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn download_does_not_create_files_through_a_planted_directory_symlink() -> Result<()> {
    let fake = FakeResults::start().await?;
    fake.seed(
        "ci-metrics-evil",
        JOB_RUN,
        OLD_JOB,
        zip_of(&[("sub/pwn.txt", b"pwn")])?,
    )?;
    let scratch = Scratch::new()?;
    let outside = scratch.workspace().join("outside");
    std::fs::create_dir_all(&outside)?;
    std::fs::create_dir_all(scratch.workspace().join("restored"))?;
    symlink(&outside, scratch.workspace().join("restored/sub"))?;
    let ran = run_job(&scratch, "artifact_download", &fake.url(), &[])?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert!(
        ran.stdout
            .contains("symbolic link outside the destination directory"),
        "{}",
        ran.stdout
    );
    assert!(!outside.join("pwn.txt").exists());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn download_replaces_a_planted_file_symlink_without_following_it() -> Result<()> {
    let fake = FakeResults::start().await?;
    fake.seed(
        "ci-metrics-a",
        JOB_RUN,
        OLD_JOB,
        zip_of(&[("m.json", b"fresh")])?,
    )?;
    let scratch = Scratch::new()?;
    write_file(&scratch, "kept.txt", b"keep")?;
    std::fs::create_dir_all(scratch.workspace().join("restored"))?;
    symlink(
        scratch.workspace().join("kept.txt"),
        scratch.workspace().join("restored/m.json"),
    )?;
    let ran = run_job(&scratch, "artifact_download", &fake.url(), &[])?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert_eq!(read_file(scratch.workspace().join("kept.txt"))?, b"keep");
    let restored = scratch.workspace().join("restored/m.json");
    assert!(
        !std::fs::symlink_metadata(&restored)?
            .file_type()
            .is_symlink()
    );
    assert_eq!(read_file(restored)?, b"fresh");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn download_follows_a_directory_symlink_that_stays_inside_the_destination() -> Result<()> {
    let fake = FakeResults::start().await?;
    fake.seed(
        "ci-metrics-a",
        JOB_RUN,
        OLD_JOB,
        zip_of(&[("alias/f.txt", b"inside")])?,
    )?;
    let scratch = Scratch::new()?;
    std::fs::create_dir_all(scratch.workspace().join("restored/real"))?;
    symlink("real", scratch.workspace().join("restored/alias"))?;
    let ran = run_job(&scratch, "artifact_download", &fake.url(), &[])?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert_eq!(
        read_file(scratch.workspace().join("restored/real/f.txt"))?,
        b"inside"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upload_follows_a_directory_symlink_and_archives_the_files_under_the_link() -> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    write_file(&scratch, "real/f.txt", b"linked")?;
    write_file(&scratch, "dist/a.txt", b"plain")?;
    symlink("../real", scratch.workspace().join("dist/sub"))?;
    let ran = run_job(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &[("name", "bundle"), ("path", "dist")],
    )?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert_eq!(
        names(&fake.artifacts()?[0].zip)?,
        [
            ("a.txt".to_owned(), b"plain".to_vec()),
            ("sub/f.txt".to_owned(), b"linked".to_vec())
        ]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upload_terminates_on_symlink_cycles() -> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    write_file(&scratch, "dist/a.txt", b"a")?;
    write_file(&scratch, "other/o.txt", b"o")?;
    symlink(".", scratch.workspace().join("dist/self"))?;
    symlink("../other", scratch.workspace().join("dist/x"))?;
    symlink("../dist", scratch.workspace().join("other/y"))?;
    let ran = run_job(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &[("name", "bundle"), ("path", "dist")],
    )?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert_eq!(
        names(&fake.artifacts()?[0].zip)?,
        [
            ("a.txt".to_owned(), b"a".to_vec()),
            ("x/o.txt".to_owned(), b"o".to_vec())
        ]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn download_refuses_an_announced_entry_count_before_parsing_the_directory() -> Result<()> {
    let fake = FakeResults::start().await?;
    fake.seed("ci-metrics-big", JOB_RUN, OLD_JOB, forged_zip(100_001))?;
    let scratch = Scratch::new()?;
    let ran = run_job(&scratch, "artifact_download", &fake.url(), &[])?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert!(
        ran.stdout
            .contains("artifact contains more than 100000 files"),
        "{}",
        ran.stdout
    );
    assert!(!scratch.workspace().join("restored").exists());
    Ok(())
}

fn at(seconds: u64) -> std::time::SystemTime {
    UNIX_EPOCH + Duration::from_secs(seconds)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upload_records_each_file_mtime_in_utc_as_the_pinned_archiver_does() -> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    let cases = [
        ("even.txt", 1_623_760_496, (2021, 6, 15, 12, 34, 56)),
        ("odd.txt", 1_623_760_497, (2021, 6, 15, 12, 34, 56)),
        ("old.txt", 86_400, (1980, 1, 1, 0, 0, 0)),
        ("far.txt", 4_102_444_800, (2043, 12, 31, 23, 59, 58)),
        ("edge.txt", 2_335_219_200, (2043, 12, 31, 23, 59, 58)),
    ];
    for (name, seconds, _) in cases {
        write_file(&scratch, name, b"x")?;
        File::options()
            .write(true)
            .open(scratch.workspace().join(name))?
            .set_modified(at(seconds))?;
    }
    let ran = run_job_env(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &[("name", "bundle"), ("path", "*.txt")],
        Env {
            process: &[("TZ", "America/Los_Angeles")],
            ..Env::default()
        },
    )?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    let mut archive = ZipArchive::new(std::io::Cursor::new(fake.artifacts()?[0].zip.clone()))?;
    for (name, _, (year, month, day, hour, minute, second)) in cases {
        let stamp = archive
            .by_name(name)?
            .last_modified()
            .context("the entry has a time")?;
        assert_eq!(
            stamp,
            DateTime::from_date_and_time(year, month, day, hour, minute, second)?,
            "{name}"
        );
    }
    Ok(())
}

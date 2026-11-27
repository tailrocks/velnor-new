// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::{FileTypeExt, symlink};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::process::Command;

use anyhow::{Result, bail};
use uuid::Uuid;

use super::artifact_support::{Env, entries_of, run_job_env, write_file};
use super::exec_support::{Ran, Scratch};
use super::fake_results::FakeResults;

const REMOVE: &str = "VELNOR_TEST_REMOVE_BEFORE_OPEN";

fn upload(scratch: &Scratch, fake: &FakeResults, process: &[(&str, &str)]) -> Result<Ran> {
    run_job_env(
        scratch,
        "artifact_upload",
        &fake.url(),
        &[("name", "bundle"), ("path", "out")],
        Env {
            process,
            ..Env::default()
        },
    )
}

fn uploaded(fake: &FakeResults) -> Result<Vec<String>> {
    Ok(entries_of(&fake.artifacts()?[0].zip)?
        .into_iter()
        .map(|(name, _)| name)
        .collect())
}

fn real(path: &Path) -> Result<String> {
    Ok(fs::canonicalize(path)?.display().to_string())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dangling_link_is_skipped_and_the_other_files_upload() -> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    write_file(&scratch, "out/ok.txt", b"ok")?;
    symlink("missing.txt", scratch.workspace().join("out/dangling.txt"))?;
    let ran = upload(&scratch, &fake, &[])?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert_eq!(uploaded(&fake)?, ["ok.txt"]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_that_vanishes_before_it_opens_is_skipped() -> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    write_file(&scratch, "out/ok.txt", b"ok")?;
    write_file(&scratch, "out/gone.txt", b"gone")?;
    let gone = real(&scratch.workspace().join("out/gone.txt"))?;
    let ran = upload(&scratch, &fake, &[(REMOVE, &gone)])?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert_eq!(uploaded(&fake)?, ["ok.txt"]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_link_target_that_vanishes_before_it_opens_is_skipped() -> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    write_file(&scratch, "out/ok.txt", b"ok")?;
    write_file(&scratch, "store/target.txt", b"target")?;
    symlink(
        scratch.workspace().join("store/target.txt"),
        scratch.workspace().join("out/link.txt"),
    )?;
    let target = real(&scratch.workspace().join("store/target.txt"))?;
    let ran = upload(&scratch, &fake, &[(REMOVE, &target)])?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert_eq!(uploaded(&fake)?, ["ok.txt"]);
    Ok(())
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_whose_name_is_not_utf8_is_skipped() -> Result<()> {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    write_file(&scratch, "out/ok.txt", b"ok")?;
    fs::write(
        scratch
            .workspace()
            .join("out")
            .join(OsStr::from_bytes(b"bad\xff.txt")),
        b"bad",
    )?;
    let ran = upload(&scratch, &fake, &[])?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert_eq!(uploaded(&fake)?, ["ok.txt"]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_matched_pipe_socket_or_link_to_a_pipe_is_skipped() -> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    write_file(&scratch, "out/ok.txt", b"ok")?;
    let out = scratch.workspace().join("out");
    if !Command::new("mkfifo")
        .arg(out.join("pipe"))
        .status()?
        .success()
    {
        bail!("mkfifo failed");
    }
    symlink("pipe", out.join("pipe-link"))?;
    let short =
        std::env::temp_dir().join(format!("s{}", &Uuid::new_v4().simple().to_string()[..12]));
    symlink(&out, &short)?;
    let _listener = UnixListener::bind(short.join("sock"))?;
    fs::remove_file(&short)?;
    assert!(
        fs::symlink_metadata(out.join("sock"))?
            .file_type()
            .is_socket()
    );
    let ran = upload(&scratch, &fake, &[])?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert_eq!(uploaded(&fake)?, ["ok.txt"]);
    let warning = "::warning::A non-blocking warning has occurred during artifact zip creation: ENTRYNOTSUPPORTED";
    assert_eq!(ran.stdout.matches(warning).count(), 3, "{}", ran.stdout);
    Ok(())
}

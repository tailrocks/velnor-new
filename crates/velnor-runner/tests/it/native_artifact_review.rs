// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use anyhow::{Context, Result};

use super::artifact_support::{
    Env, JOB_RUN, OLD_JOB, entries_of, read_file, run_job, run_job_env, spawn_job, wait_until,
    write_file, zip_of, zip_stored,
};
use super::exec_support::Scratch;
use super::fake_results::FakeResults;
use super::zip_forgery::zip_with_mode;

fn count_files(root: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(root) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => count_files(&entry.path()),
            Ok(_) => 1,
            Err(_) => 0,
        })
        .sum()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn only_the_pinned_revisions_of_the_actions_run_natively() -> Result<()> {
    let fake = FakeResults::start().await?;
    for (revision, code) in [("v3", Some(1)), ("v7", Some(0)), ("v7.0.2", Some(0))] {
        let scratch = Scratch::new()?;
        write_file(&scratch, "a.txt", b"a")?;
        let ran = run_job_env(
            &scratch,
            "artifact_upload",
            &fake.url(),
            &[("name", revision), ("path", "a.txt")],
            Env {
                revision: Some(revision),
                ..Env::default()
            },
        )?;
        assert_eq!(ran.code, code, "{revision}: {}", ran.stdout);
        if code == Some(1) {
            assert!(
                ran.has_line("unsupported: uses actions/upload-artifact@v3"),
                "{}",
                ran.stdout
            );
        }
    }
    assert_eq!(fake.calls("CreateArtifact")?.len(), 2);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unsupported_input_stops_the_job_before_the_first_step_runs() -> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    write_file(&scratch, "a.txt", b"a")?;
    let ran = run_job_env(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &[("path", "a.txt"), ("compression-level", "0")],
        Env {
            script_before: Some("echo ran > before.txt"),
            ..Env::default()
        },
    )?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert!(
        ran.has_line(
            "unsupported: actions/upload-artifact input 'compression-level' is not supported by velnor-runner"
        ),
        "{}",
        ran.stdout
    );
    assert!(!scratch.workspace().join("before.txt").exists());
    assert_eq!(fake.calls("CreateArtifact")?.len(), 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn download_does_not_restore_the_modes_recorded_in_the_zip() -> Result<()> {
    let fake = FakeResults::start().await?;
    fake.seed(
        "ci-metrics-a",
        JOB_RUN,
        OLD_JOB,
        zip_with_mode("tool.sh", b"#!/bin/sh\n", 0o755)?,
    )?;
    let scratch = Scratch::new()?;
    let ran = run_job(&scratch, "artifact_download", &fake.url(), &[])?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    let mode = std::fs::metadata(scratch.workspace().join("restored/tool.sh"))?
        .permissions()
        .mode();
    assert_eq!(mode & 0o111, 0, "{mode:o}");
    Ok(())
}

async fn download_names(patterns: &str, names: &[&str]) -> Result<(Scratch, FakeResults, String)> {
    let fake = FakeResults::start().await?;
    for name in names {
        fake.seed(
            name,
            JOB_RUN,
            OLD_JOB,
            zip_of(&[("m.json", name.as_bytes())])?,
        )?;
    }
    let scratch = Scratch::new()?;
    let ran = run_job(
        &scratch,
        "artifact_download",
        &fake.url(),
        &[("pattern", patterns), ("path", "got")],
    )?;
    Ok((scratch, fake, ran.stdout))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_star_does_not_select_artifact_names_that_start_with_a_dot() -> Result<()> {
    let (scratch, _fake, stdout) = download_names("*", &["ci-a", "ci-b", ".hidden"]).await?;
    let root = scratch.workspace().join("got");
    assert!(root.join("ci-a/m.json").exists(), "{stdout}");
    assert!(root.join("ci-b/m.json").exists(), "{stdout}");
    assert!(!root.join(".hidden").exists(), "{stdout}");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pattern_that_starts_with_a_dot_selects_dotted_names() -> Result<()> {
    let (scratch, _fake, stdout) = download_names(".hid*", &["ci-a", ".hidden"]).await?;
    assert_eq!(
        read_file(scratch.workspace().join("got/m.json"))?,
        b".hidden",
        "{stdout}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_explicitly_dotted_alternative_selects_dotted_names() -> Result<()> {
    for pattern in ["{.hidden,visible}", "[.]hidden", "{a,{.hid*,b}}"] {
        let (scratch, _fake, stdout) = download_names(pattern, &["ci-a", ".hidden"]).await?;
        assert_eq!(
            std::fs::read(scratch.workspace().join("got/m.json")).ok(),
            Some(b".hidden".to_vec()),
            "{pattern}: {stdout}"
        );
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_extglob_pattern_is_refused_instead_of_selecting_nothing() -> Result<()> {
    let (scratch, fake, stdout) = download_names("@(ci-a|ci-b)", &["ci-a", "ci-b"]).await?;
    assert!(
        stdout.contains(
            "unsupported: actions/download-artifact pattern '@(ci-a|ci-b)' uses minimatch syntax that is not supported by velnor-runner"
        ),
        "{stdout}"
    );
    assert!(!scratch.workspace().join("got").exists());
    assert_eq!(fake.calls("ListArtifacts")?.len(), 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_downloaded_archive_is_private_while_it_is_written() -> Result<()> {
    let fake = FakeResults::start().await?;
    fake.seed(
        "ci-metrics-a",
        JOB_RUN,
        OLD_JOB,
        zip_of(&[("m.json", &[b'x'; 4096])])?,
    )?;
    fake.stall_downloads_halfway()?;
    let scratch = Scratch::new()?;
    let running = spawn_job(&scratch, "artifact_download", &fake.url(), &[], &[])?;
    let found = std::cell::RefCell::new(None);
    wait_until("the temporary archive", || {
        std::fs::read_dir(scratch.temp())
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .find(|entry| entry.file_name().to_string_lossy().ends_with(".zip"))
            .and_then(|entry| entry.metadata().ok())
            .inspect(|metadata| {
                found.replace(Some(metadata.permissions().mode()));
            })
            .is_some()
    })?;
    running.stop()?;
    let mode = found.take().context("the archive was seen")?;
    assert_eq!(mode & 0o077, 0, "{mode:o}");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelling_during_extraction_stops_writing_files() -> Result<()> {
    const ENTRIES: usize = 60_000;
    let names: Vec<String> = (0..ENTRIES)
        .map(|index| format!("d{}/f{index}.txt", index % 64))
        .collect();
    let entries: Vec<(&str, &[u8])> = names
        .iter()
        .map(|name| (name.as_str(), &b"x"[..]))
        .collect();
    let fake = FakeResults::start().await?;
    fake.seed("ci-metrics-big", JOB_RUN, OLD_JOB, zip_stored(&entries)?)?;
    let scratch = Scratch::new()?;
    let running = spawn_job(&scratch, "artifact_download", &fake.url(), &[], &[])?;
    let root = scratch.workspace().join("restored");
    wait_until("the first extracted file", || count_files(&root) > 0)?;
    running.terminate()?;
    running.finish("the runner to stop")?;
    let extracted = count_files(&root);
    assert!(
        extracted < ENTRIES,
        "{extracted} of {ENTRIES} files written"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tildes_resolve_from_the_home_the_runner_inherited() -> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    let home = scratch.workspace().with_file_name("home");
    std::fs::create_dir_all(home.join("reports"))?;
    std::fs::write(home.join("reports/a.txt"), b"home")?;
    let home_text = home.to_string_lossy().into_owned();
    let env = Env {
        process: &[("HOME", &home_text)],
        ..Env::default()
    };
    let ran = run_job_env(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &[("name", "bundle"), ("path", "~/reports")],
        env,
    )?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert_eq!(
        entries_of(&fake.artifacts()?[0].zip)?,
        [("a.txt".to_owned(), b"home".to_vec())]
    );
    let ran = run_job_env(
        &scratch,
        "artifact_download",
        &fake.url(),
        &[("pattern", "bundle"), ("path", "~/restored")],
        env,
    )?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert_eq!(read_file(home.join("restored/a.txt"))?, b"home");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_download_path_with_a_leading_tilde_is_joined_to_the_home_text() -> Result<()> {
    let fake = FakeResults::start().await?;
    fake.seed("bundle", JOB_RUN, OLD_JOB, zip_of(&[("a.txt", b"home")])?)?;
    let scratch = Scratch::new()?;
    let home = scratch.workspace().with_file_name("home");
    std::fs::create_dir_all(&home)?;
    let home_text = home.to_string_lossy().into_owned();
    let ran = run_job_env(
        &scratch,
        "artifact_download",
        &fake.url(),
        &[("pattern", "bundle"), ("path", "~cache")],
        Env {
            process: &[("HOME", &home_text)],
            ..Env::default()
        },
    )?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert_eq!(read_file(format!("{home_text}cache/a.txt"))?, b"home");
    std::fs::remove_dir_all(format!("{home_text}cache"))?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_blob_put_answered_with_any_2xx_but_201_is_not_finalized() -> Result<()> {
    for status in [200_u16, 202, 204] {
        let fake = FakeResults::start().await?;
        fake.answer_uploads_with(status)?;
        let scratch = Scratch::new()?;
        write_file(&scratch, "a.txt", b"a")?;
        let ran = run_job(
            &scratch,
            "artifact_upload",
            &fake.url(),
            &[("name", "bundle"), ("path", "a.txt")],
        )?;
        assert_eq!(ran.code, Some(1), "{status}: {}", ran.stdout);
        assert!(
            ran.stdout
                .contains(&format!("artifact blob PUT failed: status={status}")),
            "{status}: {}",
            ran.stdout
        );
        assert_eq!(fake.calls("FinalizeArtifact")?.len(), 0, "{status}");
    }
    Ok(())
}

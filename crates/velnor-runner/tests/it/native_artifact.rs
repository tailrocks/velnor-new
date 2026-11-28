// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;

use super::artifact_support::{
    JOB_RUN, OLD_JOB, binary_bytes, entries_of, expires_in, fixture, output, read_file, run_job,
    write_file, zip_of,
};
use super::exec_support::{Scratch, run_path};
use super::fake_results::{FakeResults, sha256_hex};

const OTHER_JOB: &str = "00000000-0000-0000-0000-0000000000a3";
const NO_FILES: &str =
    "No files were found with the provided path: missing/*.txt. No artifacts will be uploaded.";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upload_then_download_roundtrips_files() -> Result<()> {
    let fake = FakeResults::start().await?;
    let uploader = Scratch::new()?;
    write_file(&uploader, "out/a.txt", b"alpha\n")?;
    write_file(&uploader, "out/sub/b.bin", &binary_bytes())?;
    let ran = run_job(
        &uploader,
        "artifact_upload",
        &fake.url(),
        &[("name", "bundle"), ("path", "out"), ("retention-days", "3")],
    )?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    let stored = fake.artifacts()?;
    assert_eq!(stored.len(), 1);
    assert_eq!(output(&ran, "artifact-id"), Some(stored[0].id.to_string()));
    assert_eq!(
        output(&ran, "artifact-digest"),
        Some(sha256_hex(&stored[0].zip))
    );
    let create = &fake.calls("CreateArtifact")?[0];
    assert_eq!(create["name"], "bundle");
    assert_eq!(create["workflow_run_backend_id"], JOB_RUN);
    assert_eq!(
        create["workflow_job_run_backend_id"],
        "00000000-0000-0000-0000-000000000001"
    );

    expires_in(create, 3)?;

    let downloader = Scratch::new()?;
    let ran = run_job(
        &downloader,
        "artifact_download",
        &fake.url(),
        &[("pattern", "bundle"), ("path", "got")],
    )?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    let root = downloader.workspace().join("got");
    assert_eq!(
        output(&ran, "download-path"),
        Some(root.to_string_lossy().into_owned())
    );
    assert_eq!(read_file(root.join("a.txt"))?, b"alpha\n");
    assert_eq!(read_file(root.join("sub/b.bin"))?, binary_bytes());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upload_glob_with_exclusion_selects_files() -> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    for file in [
        "src/a.rs",
        "src/b.rs",
        "src/skip.rs",
        "src/.hidden.rs",
        "src/deep/c.rs",
        "src/d.txt",
    ] {
        write_file(&scratch, file, file.as_bytes())?;
    }
    let ran = run_job(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &[("name", "sources"), ("path", "src/**/*.rs\n!src/skip.rs")],
    )?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    let names: Vec<String> = entries_of(&fake.artifacts()?[0].zip)?
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(names, ["a.rs", "b.rs", "deep/c.rs"]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upload_no_files_warn_succeeds() -> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    let ran = run_job(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &[("name", "none"), ("path", "missing/*.txt")],
    )?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert!(
        ran.has_line(&format!("::warning::{NO_FILES}")),
        "{}",
        ran.stdout
    );
    assert_eq!(fake.calls("CreateArtifact")?.len(), 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upload_no_files_error_fails() -> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    let ran = run_job(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &[
            ("name", "none"),
            ("path", "missing/*.txt"),
            ("if-no-files-found", "error"),
        ],
    )?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert!(
        ran.has_line(&format!("::error::{NO_FILES}")),
        "{}",
        ran.stdout
    );
    assert_eq!(fake.calls("CreateArtifact")?.len(), 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn overwrite_false_clash_fails() -> Result<()> {
    let fake = FakeResults::start().await?;
    let old = zip_of(&[("old.txt", b"old")])?;
    fake.seed("taken", JOB_RUN, OLD_JOB, old.clone())?;
    let scratch = Scratch::new()?;
    write_file(&scratch, "new.txt", b"new")?;
    let ran = run_job(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &[
            ("name", "taken"),
            ("path", "new.txt"),
            ("overwrite", "false"),
        ],
    )?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert!(
        ran.has_line("::error::an artifact named 'taken' already exists"),
        "{}",
        ran.stdout
    );
    assert_eq!(fake.calls("DeleteArtifact")?.len(), 0);
    assert_eq!(fake.artifacts()?[0].zip, old);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn overwrite_true_replaces_existing_artifact() -> Result<()> {
    let fake = FakeResults::start().await?;
    fake.seed(
        "ci-metrics-lane",
        JOB_RUN,
        OLD_JOB,
        zip_of(&[("old.txt", b"old")])?,
    )?;
    let scratch = Scratch::new()?;
    let metrics = scratch.options().temp.join("ci-metrics.json");
    std::fs::create_dir_all(scratch.options().temp)?;
    std::fs::write(&metrics, b"{\"new\":true}")?;
    let path = metrics.to_string_lossy();
    let ran = run_job(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &[
            ("name", "ci-metrics-lane"),
            ("path", &path),
            ("if-no-files-found", "error"),
            ("retention-days", "3"),
            ("overwrite", "true"),
        ],
    )?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    let listed = &fake.calls("ListArtifacts")?[0];
    assert_eq!(listed["name_filter"], "ci-metrics-lane");
    let deleted = &fake.calls("DeleteArtifact")?[0];
    assert_eq!(deleted["workflow_run_backend_id"], JOB_RUN);
    assert_eq!(deleted["workflow_job_run_backend_id"], OLD_JOB);
    assert_eq!(deleted["name"], "ci-metrics-lane");
    let stored = fake.artifacts()?;
    assert_eq!(stored.len(), 1);
    assert_eq!(
        entries_of(&stored[0].zip)?,
        [("ci-metrics.json".to_owned(), b"{\"new\":true}".to_vec())]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn download_single_match_extracts_into_path() -> Result<()> {
    let fake = FakeResults::start().await?;
    fake.seed(
        "ci-metrics-a",
        JOB_RUN,
        OLD_JOB,
        zip_of(&[("m.json", b"{\"a\":1}")])?,
    )?;
    fake.seed("unrelated", JOB_RUN, OLD_JOB, zip_of(&[("x.txt", b"x")])?)?;
    let scratch = Scratch::new()?;
    let ran = run_job(&scratch, "artifact_download", &fake.url(), &[])?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    let root = scratch.workspace().join("restored");
    assert_eq!(read_file(root.join("m.json"))?, b"{\"a\":1}");
    assert!(!root.join("x.txt").exists());
    assert_eq!(fake.calls("GetSignedArtifactURL")?.len(), 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn download_two_matches_extract_into_named_subdirectories() -> Result<()> {
    let fake = FakeResults::start().await?;
    fake.seed(
        "ci-metrics-a",
        JOB_RUN,
        OTHER_JOB,
        zip_of(&[("m.json", b"stale")])?,
    )?;
    fake.seed(
        "ci-metrics-a",
        JOB_RUN,
        OLD_JOB,
        zip_of(&[("m.json", b"fresh-a")])?,
    )?;
    fake.seed(
        "ci-metrics-b",
        JOB_RUN,
        OLD_JOB,
        zip_of(&[("m.json", b"fresh-b")])?,
    )?;
    let scratch = Scratch::new()?;
    let ran = run_job(&scratch, "artifact_download", &fake.url(), &[])?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    let root = scratch.workspace().join("restored");
    assert!(root.join("ci-metrics-a/m.json").exists());
    assert!(root.join("ci-metrics-b/m.json").exists());
    assert_eq!(read_file(root.join("ci-metrics-a/m.json"))?, b"fresh-a");
    assert_eq!(read_file(root.join("ci-metrics-b/m.json"))?, b"fresh-b");
    assert!(!root.join("m.json").exists());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn download_no_match_succeeds() -> Result<()> {
    let fake = FakeResults::start().await?;
    fake.seed("other", JOB_RUN, OLD_JOB, zip_of(&[("x.txt", b"x")])?)?;
    let scratch = Scratch::new()?;
    let ran = run_job(&scratch, "artifact_download", &fake.url(), &[])?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert!(
        ran.has_line("Total of 0 artifact(s) downloaded"),
        "{}",
        ran.stdout
    );
    assert_eq!(fake.calls("GetSignedArtifactURL")?.len(), 0);
    assert!(!scratch.workspace().join("restored").exists());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn download_digest_mismatch_skips_only_the_mismatching_artifact() -> Result<()> {
    let fake = FakeResults::start().await?;
    fake.seed(
        "ci-metrics-a",
        JOB_RUN,
        OLD_JOB,
        zip_of(&[("m.json", b"a")])?,
    )?;
    fake.seed(
        "ci-metrics-b",
        JOB_RUN,
        OLD_JOB,
        zip_of(&[("m.json", b"b")])?,
    )?;
    fake.set_digest(
        "ci-metrics-b",
        &format!("sha256:{}", sha256_hex(b"something else")),
    )?;
    let scratch = Scratch::new()?;
    let ran = run_job(&scratch, "artifact_download", &fake.url(), &[])?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert!(
        ran.stdout
            .contains("Digest validation failed for artifact(s): ci-metrics-b"),
        "{}",
        ran.stdout
    );
    let restored = scratch.workspace().join("restored");
    assert!(!restored.join("ci-metrics-b").exists());
    assert_eq!(read_file(restored.join("ci-metrics-a/m.json"))?, b"a");
    Ok(())
}

#[test]
fn unsupported_input_fails_with_message() -> Result<()> {
    let ran = run_path(&fixture("artifact_unsupported_input"), &Scratch::new()?)?;
    assert!(
        ran.has_line(
            "unsupported: actions/upload-artifact input 'include-hidden-files' is not supported by velnor-runner"
        ),
        "{}",
        ran.stdout
    );
    assert_eq!(ran.code, Some(1));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn zip_entry_escaping_destination_is_rejected() -> Result<()> {
    let fake = FakeResults::start().await?;
    let hostile = zip_of(&[("fine.txt", b"fine"), ("../escape.txt", b"escaped")])?;
    fake.seed("ci-metrics-evil", JOB_RUN, OLD_JOB, hostile)?;
    let scratch = Scratch::new()?;
    let ran = run_job(&scratch, "artifact_download", &fake.url(), &[])?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert!(
        ran.stdout.contains("outside the destination directory"),
        "{}",
        ran.stdout
    );
    assert!(!scratch.workspace().join("escape.txt").exists());
    assert!(!scratch.workspace().join("restored").exists());
    Ok(())
}

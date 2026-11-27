// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::artifact_support::{
    Env, JOB_RUN, OLD_JOB, entries_of, error_lines, expires_in, read_file, run_job, run_job_env,
    write_file, zip_of,
};
use super::exec_support::Scratch;
use super::fake_results::FakeResults;
use super::native_artifact_texts::name_error;
use super::support::Reply;
use super::zip_forgery::{announcing_sizes, zip_with_symlink};
use anyhow::Result;
use serde_json::json;

const ANOTHER_RUN: &str = "00000000-0000-0000-0000-0000000000a1";
const NO_FILES: &str =
    "No files were found with the provided path: missing/*.txt. No artifacts will be uploaded.";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retention_days_are_capped_by_the_repository_maximum() -> Result<()> {
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
            ("retention-days", "5"),
        ],
        Env {
            process: &[],
            job: &[("GITHUB_RETENTION_DAYS", "2")],
            ..Env::default()
        },
    )?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert!(
        ran.has_line("::warning::Retention days cannot be greater than the maximum allowed retention set within the repository. Using 2 instead."),
        "{}",
        ran.stdout
    );
    expires_in(&fake.calls("CreateArtifact")?[0], 2)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upload_rejects_an_unsafe_name_before_any_request() -> Result<()> {
    let rows = [
        ("a/b", name_error("a/b", "Forward slash /")),
        ("a\\b", name_error("a\\b", "Backslash \\")),
        ("a:b", name_error("a:b", "Colon :")),
        ("a:\0b", name_error("a:\0b", "Colon :")),
        (
            "a\0b",
            "::error::The artifact name is not valid: a\0b. Velnor refuses a null character in a name, which the toolkit does not check.".to_owned(),
        ),
    ];
    for (name, expected) in rows {
        let fake = FakeResults::start().await?;
        let scratch = Scratch::new()?;
        write_file(&scratch, "a.txt", b"a")?;
        let ran = run_job(
            &scratch,
            "artifact_upload",
            &fake.url(),
            &[("name", name), ("path", "a.txt")],
        )?;
        assert_eq!(ran.code, Some(1), "{name:?}: {}", ran.stdout);
        let lines = error_lines(&ran.stdout);
        assert_eq!(lines, [expected.as_str()], "{name:?}");
        assert_eq!(fake.calls("CreateArtifact")?.len(), 0, "{name:?}");
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn download_refuses_a_listed_name_that_climbs_out_of_the_destination() -> Result<()> {
    let fake = FakeResults::start().await?;
    fake.seed("..", JOB_RUN, OLD_JOB, zip_of(&[("m.json", b"escaped")])?)?;
    fake.seed("fine", JOB_RUN, OLD_JOB, zip_of(&[("m.json", b"fine")])?)?;
    let scratch = Scratch::new()?;
    let ran = run_job(
        &scratch,
        "artifact_download",
        &fake.url(),
        &[("pattern", "{..,fine}"), ("path", "restored")],
    )?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert!(
        ran.stdout
            .contains("artifact name '..' cannot be extracted into a directory of its own"),
        "{}",
        ran.stdout
    );
    assert!(!scratch.workspace().join("m.json").exists());
    assert!(!scratch.workspace().join("restored").exists());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_files_found_ignore_succeeds_without_a_warning_or_error() -> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    let ran = run_job(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &[
            ("name", "none"),
            ("path", "missing/*.txt"),
            ("if-no-files-found", "ignore"),
        ],
    )?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert!(ran.has_line(NO_FILES), "{}", ran.stdout);
    assert!(
        !ran.stdout.contains("::warning::") && !ran.stdout.contains("::error::"),
        "{}",
        ran.stdout
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_files_found_with_an_unknown_value_fails_with_the_upstream_text() -> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    let ran = run_job(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &[
            ("name", "none"),
            ("path", "missing/*.txt"),
            ("if-no-files-found", "bogus"),
        ],
    )?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert!(
        ran.has_line("::error::Unrecognized if-no-files-found input. Provided: bogus. Available options: warn,error,ignore"),
        "{}",
        ran.stdout
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn download_refuses_a_zip_announcing_more_than_the_extraction_limit() -> Result<()> {
    let fake = FakeResults::start().await?;
    let zip = zip_of(&[("a", b"a"), ("b", b"b"), ("c", b"c")])?;
    fake.seed(
        "ci-metrics-big",
        JOB_RUN,
        OLD_JOB,
        announcing_sizes(zip, 1 << 31),
    )?;
    let scratch = Scratch::new()?;
    let ran = run_job(&scratch, "artifact_download", &fake.url(), &[])?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert!(ran.stdout.contains("extraction limit"), "{}", ran.stdout);
    assert!(!scratch.workspace().join("restored").exists());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn download_refuses_a_zip_with_a_symlink_entry() -> Result<()> {
    let fake = FakeResults::start().await?;
    fake.seed(
        "ci-metrics-link",
        JOB_RUN,
        OLD_JOB,
        zip_with_symlink("link", "/etc/passwd")?,
    )?;
    let scratch = Scratch::new()?;
    let ran = run_job(&scratch, "artifact_download", &fake.url(), &[])?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert!(ran.stdout.contains("is a symbolic link"), "{}", ran.stdout);
    assert!(!scratch.workspace().join("restored").exists());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upload_of_files_in_sibling_directories_keeps_their_directories() -> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    write_file(&scratch, "a/x.txt", b"x")?;
    write_file(&scratch, "b/y.txt", b"y")?;
    let ran = run_job(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &[("name", "bundle"), ("path", "a/x.txt\nb/y.txt")],
    )?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert_eq!(
        entries_of(&fake.artifacts()?[0].zip)?,
        [
            ("a/x.txt".to_owned(), b"x".to_vec()),
            ("b/y.txt".to_owned(), b"y".to_vec())
        ]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn overwrite_deletes_the_listed_artifact_with_its_own_ids_and_no_other_run() -> Result<()> {
    let fake = FakeResults::start().await?;
    fake.seed("bundle", JOB_RUN, OLD_JOB, zip_of(&[("old.txt", b"old")])?)?;
    fake.seed(
        "bundle",
        ANOTHER_RUN,
        OLD_JOB,
        zip_of(&[("kept.txt", b"kept")])?,
    )?;
    let scratch = Scratch::new()?;
    write_file(&scratch, "a.txt", b"a")?;
    let ran = run_job(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &[("name", "bundle"), ("path", "a.txt"), ("overwrite", "true")],
    )?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    let deleted = fake.calls("DeleteArtifact")?;
    assert_eq!(deleted.len(), 1);
    assert_eq!(deleted[0]["workflow_run_backend_id"], JOB_RUN);
    assert_eq!(deleted[0]["workflow_job_run_backend_id"], OLD_JOB);
    let kept = fake
        .artifacts()?
        .into_iter()
        .filter(|stored| stored.run == ANOTHER_RUN)
        .count();
    assert_eq!(kept, 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn overwrite_with_nothing_stored_still_uploads() -> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    write_file(&scratch, "a.txt", b"a")?;
    let ran = run_job(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &[("name", "bundle"), ("path", "a.txt"), ("overwrite", "true")],
    )?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert_eq!(fake.artifacts()?.len(), 1);
    assert_eq!(fake.calls("DeleteArtifact")?.len(), 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_download_installs_each_batch_before_it_fetches_the_next() -> Result<()> {
    let fake = FakeResults::start().await?;
    for number in 1..=7 {
        fake.seed(
            &format!("ci-metrics-{number}"),
            JOB_RUN,
            OLD_JOB,
            zip_of(&[("m.json", format!("a{number}").as_bytes())])?,
        )?;
    }
    fake.lose_blob("ci-metrics-1")?;
    let scratch = Scratch::new()?;
    let ran = run_job(&scratch, "artifact_download", &fake.url(), &[])?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert!(
        ran.has_line("::error::Unable to download artifact(s): Unable to download and extract artifact: artifact blob GET failed: status=404 Not Found"),
        "{}",
        ran.stdout
    );
    let restored = scratch.workspace().join("restored");
    for number in 3..=7 {
        assert_eq!(
            std::fs::read(restored.join(format!("ci-metrics-{number}/m.json")))?,
            format!("a{number}").as_bytes()
        );
    }
    assert!(!restored.join("ci-metrics-2").exists());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_digest_mismatch_in_one_batch_does_not_stop_the_next_batch_from_installing() -> Result<()>
{
    let fake = FakeResults::start().await?;
    for number in 1..=7 {
        fake.seed(
            &format!("ci-metrics-{number}"),
            JOB_RUN,
            OLD_JOB,
            zip_of(&[("m.json", b"x")])?,
        )?;
    }
    let wrong = format!("sha256:{}", super::fake_results::sha256_hex(b"other"));
    fake.set_digest("ci-metrics-7", &wrong)?;
    fake.set_digest("ci-metrics-1", &wrong)?;
    let scratch = Scratch::new()?;
    let ran = run_job(&scratch, "artifact_download", &fake.url(), &[])?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert!(
        ran.has_line("::error::Unable to download artifact(s): Digest validation failed for artifact(s): ci-metrics-7, ci-metrics-1"),
        "{}",
        ran.stdout
    );
    let restored = scratch.workspace().join("restored");
    for number in 2..=6 {
        assert_eq!(
            read_file(restored.join(format!("ci-metrics-{number}/m.json")))?,
            b"x"
        );
    }
    assert!(!restored.join("ci-metrics-7").exists() && !restored.join("ci-metrics-1").exists());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failing_blob_download_does_not_print_the_signed_url_query() -> Result<()> {
    let fake = FakeResults::start().await?;
    fake.seed(
        "ci-metrics-a",
        JOB_RUN,
        OLD_JOB,
        zip_of(&[("m.json", b"a")])?,
    )?;
    let denied = format!("{}/denied?sig=topsecret", fake.url());
    fake.script(
        "GetSignedArtifactURL",
        vec![Reply::json(200, &json!({ "signed_url": denied }))],
    )?;
    let scratch = Scratch::new()?;
    let ran = run_job(&scratch, "artifact_download", &fake.url(), &[])?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert!(
        !ran.stdout.contains("topsecret") && !ran.stderr.contains("topsecret"),
        "{}{}",
        ran.stdout,
        ran.stderr
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn camel_case_response_fields_are_accepted_like_the_toolkit_json_reader() -> Result<()> {
    let fake = FakeResults::start().await?;
    fake.respond_in_camel_case()?;
    let uploader = Scratch::new()?;
    write_file(&uploader, "a.txt", b"camel")?;
    let ran = run_job(
        &uploader,
        "artifact_upload",
        &fake.url(),
        &[("name", "bundle"), ("path", "a.txt")],
    )?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    let downloader = Scratch::new()?;
    let ran = run_job(
        &downloader,
        "artifact_download",
        &fake.url(),
        &[("pattern", "bundle"), ("path", "restored")],
    )?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert_eq!(
        read_file(downloader.workspace().join("restored/a.txt"))?,
        b"camel"
    );
    Ok(())
}

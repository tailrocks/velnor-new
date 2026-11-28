// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::{PermissionsExt, symlink};

use anyhow::Result;

use super::artifact_support::{entries_of, error_lines, run_job, spawn_job, write_file};
use super::exec_support::Scratch;
use super::fake_results::FakeResults;
use super::native_artifact_texts::path_error;

fn upload(scratch: &Scratch, fake: &FakeResults, path: &str) -> Result<super::exec_support::Ran> {
    run_job(
        scratch,
        "artifact_upload",
        &fake.url(),
        &[("name", "bundle"), ("path", path)],
    )
}

fn names(fake: &FakeResults) -> Result<Vec<String>> {
    Ok(entries_of(&fake.artifacts()?[0].zip)?
        .into_iter()
        .map(|(name, _)| name)
        .collect())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_lone_dot_uploads_the_workspace() -> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    write_file(&scratch, "a.txt", b"a")?;
    write_file(&scratch, "sub/b.txt", b"b")?;
    let ran = upload(&scratch, &fake, ".")?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert_eq!(names(&fake)?, ["a.txt", "sub/b.txt"]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unreadable_hidden_directory_is_pruned_not_walked() -> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    write_file(&scratch, "out/a.txt", b"a")?;
    write_file(&scratch, "out/.cache/c.txt", b"c")?;
    let hidden = scratch.workspace().join("out/.cache");
    std::fs::set_permissions(&hidden, std::fs::Permissions::from_mode(0o000))?;
    let ran = upload(&scratch, &fake, "out")?;
    std::fs::set_permissions(&hidden, std::fs::Permissions::from_mode(0o755))?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert_eq!(names(&fake)?, ["a.txt"]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn several_roots_keep_the_declared_path_of_a_symlinked_parent() -> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    write_file(&scratch, "real/out/a.txt", b"a")?;
    write_file(&scratch, "reports/r.txt", b"r")?;
    symlink("real", scratch.workspace().join("link"))?;
    let ran = upload(&scratch, &fake, "link/out\nreports")?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert_eq!(names(&fake)?, ["link/out/a.txt", "reports/r.txt"]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelling_a_search_that_matches_nothing_stops_the_walk() -> Result<()> {
    const LEVELS: usize = 28;
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    for level in 0..LEVELS {
        std::fs::create_dir_all(scratch.workspace().join(format!("fan/n{level}")))?;
    }
    for level in 0..LEVELS - 1 {
        for name in ["a", "b"] {
            symlink(
                format!("../n{}", level + 1),
                scratch.workspace().join(format!("fan/n{level}/{name}")),
            )?;
        }
    }
    let running = spawn_job(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &[("name", "bundle"), ("path", "fan/**/*.nomatch")],
        &[("VELNOR_CANCEL_GRACE_MS", "900000")],
    )?;
    running.wait_for_line("step 1 start: actions/upload-artifact")?;
    running.terminate()?;
    running.finish("the runner to stop")?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_link_to_another_root_archives_the_files_under_both_names() -> Result<()> {
    let fake = FakeResults::start().await?;
    let scratch = Scratch::new()?;
    write_file(&scratch, "a/x/f.txt", b"f")?;
    symlink("a", scratch.workspace().join("link"))?;
    let ran = upload(&scratch, &fake, "a/\nlink/x/")?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert_eq!(names(&fake)?, ["a/x/f.txt", "link/x/f.txt"]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_path_the_toolkit_forbids_is_refused_before_the_artifact_is_created() -> Result<()> {
    let cases = [
        ("report:latest.txt", "Colon :"),
        ("result?.json", "Question mark ?"),
        ("a\"b.txt", "Double quote \""),
        ("a<b.txt", "Less than <"),
        ("a|b.txt", "Vertical bar |"),
    ];
    for (file, character) in cases {
        let fake = FakeResults::start().await?;
        let scratch = Scratch::new()?;
        write_file(&scratch, "ok.txt", b"ok")?;
        write_file(&scratch, file, b"x")?;
        let ran = upload(&scratch, &fake, ".")?;
        assert_eq!(ran.code, Some(1), "{file}: {}", ran.stdout);
        let expected = path_error(&format!("/{file}"), character);
        assert_eq!(error_lines(&ran.stdout), [expected.as_str()], "{file}");
        assert_eq!(fake.calls("CreateArtifact")?.len(), 0, "{file}");
    }
    Ok(())
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use super::super::fixture::TestResult;
use super::MBX_DECLARED;
use super::scope::{assert_plain, assert_refused, declared_job, run_with_args};

#[test]
fn a_directory_flag_names_the_project_the_command_runs_in() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let declaring = repo.root().display().to_string();
    let scratch = repo.root().join("scratch-project").display().to_string();
    assert_refused(&run_with_args(
        &repo,
        &launch,
        &["-C", &declaring, "build"],
    )?);
    assert_plain(&run_with_args(&repo, &launch, &["-C", &scratch, "build"])?);
    Ok(())
}

#[test]
fn a_direct_script_target_is_scoped_by_its_path() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let inside = repo.root().join("tool.rs").display().to_string();
    let outside = repo
        .root()
        .join("scratch-project/tool.rs")
        .display()
        .to_string();
    assert_refused(&run_with_args(
        &repo,
        &launch,
        &["+nightly", "-Zscript", &inside],
    )?);
    assert_plain(&run_with_args(
        &repo,
        &launch,
        &["+nightly", "-Zscript", &outside],
    )?);
    Ok(())
}

#[test]
fn an_attached_directory_flag_names_the_project_the_command_runs_in() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let declaring = format!("-C{}", repo.root().display());
    let scratch = format!("-C{}", repo.root().join("scratch-project").display());
    assert_refused(&run_with_args(&repo, &launch, &[&declaring, "build"])?);
    assert_plain(&run_with_args(&repo, &launch, &[&scratch, "build"])?);
    Ok(())
}

#[test]
fn each_directory_flag_is_applied_in_order_like_cargo() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let scratch = repo.root().join("scratch-project").display().to_string();
    let declaring = repo.root().display().to_string();
    assert_refused(&run_with_args(
        &repo,
        &launch,
        &[&format!("-C{scratch}"), "-C", &declaring, "build"],
    )?);
    assert_plain(&run_with_args(
        &repo,
        &launch,
        &["-C", &declaring, &format!("-C{scratch}"), "build"],
    )?);
    Ok(())
}

#[test]
fn an_equals_attached_directory_flag_names_the_project_the_command_runs_in() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let declaring = format!("-C={}", repo.root().display());
    let scratch = format!("-C={}", repo.root().join("scratch-project").display());
    assert_refused(&run_with_args(&repo, &launch, &[&declaring, "build"])?);
    assert_plain(&run_with_args(&repo, &launch, &[&scratch, "build"])?);
    Ok(())
}

#[test]
fn an_extensionless_script_target_is_scoped_by_its_path() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    fs::create_dir_all(repo.root().join("scratch-project"))?;
    let inside = repo.root().join("tool").display().to_string();
    let outside = repo
        .root()
        .join("scratch-project/tool")
        .display()
        .to_string();
    for args in [
        ["+nightly", "-Zscript", &inside],
        ["+nightly", "-Zscript", "../tool"],
    ] {
        assert_refused(&run_with_args(&repo, &launch, &args)?);
    }
    for args in [
        ["+nightly", "-Zscript", &outside],
        ["+nightly", "-Zscript", "../scratch-project/tool"],
    ] {
        assert_plain(&run_with_args(&repo, &launch, &args)?);
    }
    Ok(())
}

#[test]
fn a_nested_script_target_is_scoped_by_its_path() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    fs::create_dir_all(launch.join("a/b"))?;
    fs::create_dir_all(launch.join("a/x"))?;
    fs::write(launch.join("a/b/mise.toml"), MBX_DECLARED)?;
    assert_refused(&run_with_args(
        &repo,
        &launch,
        &["+nightly", "-Zscript", "a/b/c"],
    )?);
    assert_plain(&run_with_args(
        &repo,
        &launch,
        &["+nightly", "-Zscript", "a/x/c"],
    )?);
    Ok(())
}

#[test]
fn a_script_target_with_dot_and_dot_dot_components_is_scoped_by_where_it_lands() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    fs::create_dir_all(launch.join("deep"))?;
    fs::create_dir_all(repo.root().join("scratch-project"))?;
    assert_refused(&run_with_args(
        &repo,
        &launch,
        &["+nightly", "-Zscript", "./deep/../../tool"],
    )?);
    assert_refused(&run_with_args(
        &repo,
        &launch,
        &["+nightly", "-Z", "script", "./../tool"],
    )?);
    assert_plain(&run_with_args(
        &repo,
        &launch,
        &["+nightly", "-Zscript", "./deep/../../scratch-project/tool"],
    )?);
    Ok(())
}

#[test]
fn a_script_target_that_cannot_be_resolved_fails_closed() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    assert_refused(&run_with_args(
        &repo,
        &launch,
        &["+nightly", "-Zscript", "missing/../../tool"],
    )?);
    Ok(())
}

#[test]
fn an_external_subcommand_under_zscript_is_scoped_by_its_manifest_path() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let inside = repo.root().join("Cargo.toml").display().to_string();
    let outside = repo
        .root()
        .join("scratch-project/Cargo.toml")
        .display()
        .to_string();
    for command in [&["nextest", "run"][..], &["foo"][..]] {
        let mut declared = vec!["+nightly", "-Zscript"];
        declared.extend_from_slice(command);
        declared.extend(["--manifest-path", &inside]);
        assert_refused(&run_with_args(&repo, &launch, &declared)?);
        let mut scratch = vec!["+nightly", "-Zscript"];
        scratch.extend_from_slice(command);
        scratch.extend(["--manifest-path", &outside]);
        assert_plain(&run_with_args(&repo, &launch, &scratch)?);
    }
    Ok(())
}

#[test]
fn a_built_in_command_under_zscript_is_scoped_by_its_manifest_path() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let inside = repo.root().join("Cargo.toml").display().to_string();
    let outside = repo
        .root()
        .join("scratch-project/Cargo.toml")
        .display()
        .to_string();
    assert_refused(&run_with_args(
        &repo,
        &launch,
        &["+nightly", "-Zscript", "build", "--manifest-path", &inside],
    )?);
    assert_plain(&run_with_args(
        &repo,
        &launch,
        &["+nightly", "-Zscript", "build", "-m", &outside],
    )?);
    assert_refused(&run_with_args(
        &repo,
        repo.root(),
        &["+nightly", "-Zscript", "build"],
    )?);
    Ok(())
}

#[test]
fn a_bare_name_under_zscript_is_a_subcommand_and_a_dot_slash_name_is_a_script() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    fs::create_dir_all(repo.root().join("scratch-project"))?;
    let inside = repo.root().join("Cargo.toml").display().to_string();
    assert_refused(&run_with_args(
        &repo,
        &launch,
        &["+nightly", "-Zscript", "tool", "--manifest-path", &inside],
    )?);
    assert_refused(&run_with_args(
        &repo,
        &launch,
        &[
            "+nightly",
            "-Zscript",
            "../tool",
            "--manifest-path",
            &inside,
        ],
    )?);
    assert_plain(&run_with_args(
        &repo,
        &launch,
        &[
            "+nightly",
            "-Zscript",
            "../scratch-project/tool",
            "--manifest-path",
            &inside,
        ],
    )?);
    Ok(())
}

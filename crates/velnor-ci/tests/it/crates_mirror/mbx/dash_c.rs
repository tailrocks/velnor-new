// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use super::super::fixture::TestResult;
use super::scope::{assert_plain, assert_refused, declared_job, run_with_args};
use super::{MBX_DECLARED, plain_cargo, wrapper_command};

#[test]
fn a_dash_c_directory_names_the_project_for_a_build_started_outside_it() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let checkout = repo.root().display().to_string();
    assert_refused(&run_with_args(&repo, &launch, &["-C", &checkout, "build"])?);
    let outside = repo.root().join("scratch-project");
    fs::create_dir_all(&outside)?;
    let outside = outside.display().to_string();
    assert_plain(&run_with_args(&repo, &launch, &["-C", &outside, "build"])?);
    Ok(())
}

#[test]
fn every_dash_c_spelling_names_the_project_it_changes_into() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let scratch = repo.root().join("scratch-project");
    fs::create_dir_all(&scratch)?;
    for (directory, declared) in [
        (repo.root().display().to_string(), true),
        (scratch.display().to_string(), false),
    ] {
        let spellings: [Vec<String>; 9] = [
            vec!["-C".into(), directory.clone(), "build".into()],
            vec![
                "+stable".into(),
                "-C".into(),
                directory.clone(),
                "build".into(),
            ],
            vec![format!("-C{directory}"), "build".into()],
            vec![format!("-C={directory}"), "build".into()],
            vec![format!("-vC{directory}"), "build".into()],
            vec![format!("-qC={directory}"), "build".into()],
            vec!["-vC".into(), directory.clone(), "build".into()],
            vec![
                "-Z".into(),
                "unstable-options".into(),
                format!("-C{directory}"),
                "build".into(),
            ],
            vec![
                "--color=never".into(),
                "-C".into(),
                directory.clone(),
                "build".into(),
            ],
        ];
        for spelling in &spellings {
            let args: Vec<&str> = spelling.iter().map(String::as_str).collect();
            let output = run_with_args(&repo, &launch, &args)?;
            if declared {
                assert_refused(&output);
            } else {
                assert_plain(&output);
            }
        }
    }
    Ok(())
}

#[test]
fn a_global_option_list_cargo_would_reject_requires_mbx() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let scratch_dir = repo.root().join("scratch-project");
    fs::create_dir_all(&scratch_dir)?;
    let scratch = scratch_dir.display().to_string();
    let attached = format!("-C{scratch}");
    let twice = ["-C", scratch.as_str(), attached.as_str(), "build"];
    let rejected: [&[&str]; 7] = [
        &["-C"],
        &["-C=", "build"],
        &["-C", "", "build"],
        &twice,
        &["--bogus", "build"],
        &["-x", "build"],
        &["--color"],
    ];
    for args in rejected {
        assert_refused(&run_with_args(&repo, &launch, args)?);
    }
    Ok(())
}

#[test]
fn a_dash_c_symlink_resolves_to_the_directory_whose_config_expands_the_alias() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    fs::create_dir_all(&launch)?;
    let deep = repo.root().join("scratch-project/deep");
    fs::create_dir_all(&deep)?;
    let config = repo.root().join("scratch-project/.cargo");
    fs::create_dir_all(&config)?;
    let manifest = repo.root().join("Cargo.toml").display().to_string();
    fs::write(
        config.join("config.toml"),
        format!("[alias]\nfoo = \"build --manifest-path {manifest}\"\n"),
    )?;
    std::os::unix::fs::symlink(&deep, launch.join("link"))?;
    assert_refused(&run_with_args(&repo, &launch, &["-C", "link", "foo"])?);
    Ok(())
}

#[test]
fn a_relative_manifest_path_resolves_against_the_dash_c_directory() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    fs::create_dir_all(repo.root().join("scratch-project"))?;
    let run = |directory: &str| -> Result<std::process::Output, Box<dyn std::error::Error>> {
        run_with_args(
            &repo,
            &launch,
            &["-C", directory, "metadata", "--manifest-path", "Cargo.toml"],
        )
    };
    assert_refused(&run("..")?);
    assert_plain(&run("../scratch-project")?);
    Ok(())
}

#[test]
fn a_dash_c_directory_that_does_not_exist_fails_closed() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    assert_refused(&run_with_args(
        &repo,
        &launch,
        &["-C", "missing-directory", "build"],
    )?);
    Ok(())
}

#[test]
fn a_dash_c_into_a_member_of_a_declaring_workspace_walks_up_to_the_workspace() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let workspace = repo.root().join("workspace");
    let member = workspace.join("crates/member");
    fs::create_dir_all(&member)?;
    fs::create_dir_all(&launch)?;
    fs::write(workspace.join("mise.toml"), MBX_DECLARED.as_str())?;
    let plain = plain_cargo(&repo)?;
    let output = wrapper_command(repo.root(), &plain, false, &[])?
        .current_dir(&launch)
        .env("VELNOR_CARGO_MBX", "required")
        .env("GITHUB_WORKSPACE", &workspace)
        .args(["-C", "../workspace/crates/member", "build"])
        .output()?;
    assert_refused(&output);
    Ok(())
}

#[test]
fn a_dash_c_after_a_double_dash_is_not_a_global_option() -> TestResult {
    let repo = declared_job()?;
    let launch = repo.root().join("launch");
    let scratch = repo.root().join("scratch-project");
    fs::create_dir_all(&scratch)?;
    let checkout = repo.root().display().to_string();
    let scratch = scratch.display().to_string();
    assert_plain(&run_with_args(
        &repo,
        &launch,
        &["--", "-C", &checkout, "build"],
    )?);
    assert_refused(&run_with_args(
        &repo,
        repo.root(),
        &["--", "-C", &scratch, "build"],
    )?);
    Ok(())
}

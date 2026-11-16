// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use velnor_gen::cli::{Command as Cmd, ReleaseCommand};
use velnor_gen::io::parse_cli;
use velnor_gen::{GenerateError, release_ruleset};
use velnor_model::RepoPath;

use crate::golden::{load, samples};
use crate::release_fixture::{APP, Root, release_view};

type TestResult = Result<(), Box<dyn Error>>;

fn golden() -> Result<String, Box<dyn Error>> {
    Ok(fs::read_to_string(
        samples().join("release-two-roots/ruleset.json"),
    )?)
}

#[test]
fn ruleset_on_the_golden_prints_the_recorded_json() -> TestResult {
    let view = load(&samples().join("release-two-roots"))?;
    let json = release_ruleset(&view)?;
    if std::env::var_os("UPDATE_GOLDENS").is_some() {
        fs::write(samples().join("release-two-roots/ruleset.json"), &json)?;
    }
    assert_eq!(json, golden()?);
    Ok(())
}

#[test]
fn ruleset_lets_only_the_release_app_create_the_tags_the_release_plz_files_name() -> TestResult {
    let trees: serde_json::Value = serde_json::from_str(&golden()?)?;
    assert_eq!(trees.as_array().map(Vec::len), Some(2));
    let tree = &trees[0];
    assert_eq!(tree["name"], "release-tags");
    assert_eq!(tree["target"], "tag");
    assert_eq!(tree["enforcement"], "active");
    assert_eq!(
        tree["conditions"]["ref_name"]["include"],
        serde_json::json!(["refs/tags/*-v*", "refs/tags/app-v*", "refs/tags/tools-v*"])
    );
    assert_eq!(
        tree["conditions"]["ref_name"]["exclude"],
        serde_json::json!([])
    );
    assert_eq!(tree["rules"], serde_json::json!([{ "type": "creation" }]));
    assert_eq!(
        tree["bypass_actors"],
        serde_json::json!([{
            "actor_id": 5_762_496,
            "actor_type": "Integration",
            "bypass_mode": "always"
        }])
    );
    let immutable = &trees[1];
    assert_eq!(immutable["name"], "release-tags-immutable");
    assert_eq!(
        immutable["rules"],
        serde_json::json!([{ "type": "update" }, { "type": "deletion" }])
    );
    assert_eq!(immutable["bypass_actors"], serde_json::json!([]));
    assert_eq!(immutable["conditions"], tree["conditions"]);
    Ok(())
}

#[test]
fn ruleset_needs_a_release_config_and_a_valid_plan() -> TestResult {
    let mut view = release_view(&[APP], None)?;
    assert_eq!(
        release_ruleset(&view).err(),
        Some(GenerateError::NoReleaseConfig)
    );
    view = release_view(&[], Some(""))?;
    assert!(matches!(
        release_ruleset(&view),
        Err(GenerateError::Release(_))
    ));
    Ok(())
}

#[test]
fn ruleset_refuses_a_published_package_that_names_another_registry() -> TestResult {
    let root = Root {
        publishes: true,
        ..APP
    };
    let mut view = release_view(&[root], Some(""))?;
    view.files.insert(
        RepoPath::new("Cargo.toml")?,
        b"[package]\nname = \"app\"\nversion = \"0.1.0\"\npublish = [\"company\"]\n".to_vec(),
    );
    assert!(matches!(
        release_ruleset(&view),
        Err(GenerateError::Release(
            velnor_release::ReleaseError::PublishNamesOtherRegistry { .. }
        ))
    ));
    Ok(())
}

#[test]
fn the_release_ruleset_command_parses() -> TestResult {
    let cli = parse_cli(["velnor-gen", "release", "ruleset"])?;
    assert_eq!(cli.command, Cmd::Release(ReleaseCommand::Ruleset));
    assert!(parse_cli(["velnor-gen", "release"]).is_err());
    Ok(())
}

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

fn copy_tree(from: &Path, to: &Path) -> TestResult {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn git(directory: &Path, args: &[&str]) -> TestResult {
    let status = Command::new(crate::setup_action::program("git"))
        .args(args)
        .current_dir(directory)
        .status()?;
    assert!(status.success(), "git {args:?}");
    Ok(())
}

#[test]
fn the_binary_prints_the_golden_ruleset_for_a_repository_that_holds_the_sample() -> TestResult {
    let repo =
        Scratch(std::env::temp_dir().join(format!("velnor-gen-ruleset-{}", std::process::id())));
    fs::remove_dir_all(&repo.0).ok();
    copy_tree(&samples().join("release-two-roots/view"), &repo.0)?;
    git(&repo.0, &["init", "--quiet"])?;
    git(&repo.0, &["add", "--all"])?;
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-gen"))
        .args(["--root", ".", "release", "ruleset"])
        .current_dir(&repo.0)
        .env("PATH", "/usr/bin:/bin")
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout)?, golden()?);
    let paths: BTreeSet<String> = [
        ".velnor/release.toml",
        "release-plz.toml",
        "tools/release-plz.toml",
    ]
    .map(str::to_owned)
    .into();
    for path in &paths {
        assert!(
            repo.0.join(RepoPath::new(path)?.as_str()).is_file(),
            "{path}"
        );
    }
    Ok(())
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use velnor_gen::cli::{Command as Cmd, ReleaseCommand};
use velnor_gen::io::parse_cli;
use velnor_gen::{GenerateError, release_ruleset, release_ruleset_drift};
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
    let trees: serde_json::Value = golden()?
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<Vec<serde_json::Value>, _>>()?
        .into();
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
fn ruleset_prints_one_request_body_per_line() -> TestResult {
    let json = golden()?;
    let lines: Vec<&str> = json.lines().collect();
    assert_eq!(lines.len(), 2);
    for line in lines {
        let body: serde_json::Value = serde_json::from_str(line)?;
        assert!(body.is_object(), "{line}");
        assert!(
            body["name"].is_string() && body["rules"].is_array(),
            "{line}"
        );
    }
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

fn live() -> Result<serde_json::Value, Box<dyn Error>> {
    Ok(serde_json::from_str(&fs::read_to_string(
        samples().join("release-two-roots/live-rulesets.json"),
    )?)?)
}

fn drift(live: &serde_json::Value) -> Result<Vec<String>, Box<dyn Error>> {
    let view = load(&samples().join("release-two-roots"))?;
    Ok(release_ruleset_drift(&view, &live.to_string())?)
}

#[test]
fn the_recorded_admin_response_of_the_generated_rulesets_has_no_drift() -> TestResult {
    assert_eq!(drift(&live()?)?, Vec::<String>::new());
    Ok(())
}

#[test]
fn a_bypass_actor_added_to_the_immutability_ruleset_is_reported() -> TestResult {
    let mut response = live()?;
    response[1]["bypass_actors"] = serde_json::json!([{
        "actor_id": 5_762_496,
        "actor_type": "Integration",
        "bypass_mode": "always"
    }]);
    let drifts = drift(&response)?;
    assert_eq!(drifts.len(), 1, "{drifts:?}");
    assert!(
        drifts[0].starts_with("release-tags-immutable: bypass_actors differ"),
        "{drifts:?}"
    );
    Ok(())
}

#[test]
fn a_response_without_bypass_actors_is_reported_not_trusted() -> TestResult {
    let mut response = live()?;
    for ruleset in response.as_array_mut().into_iter().flatten() {
        ruleset
            .as_object_mut()
            .map(|object| object.remove("bypass_actors"));
    }
    let drifts = drift(&response)?;
    assert_eq!(drifts.len(), 2, "{drifts:?}");
    assert!(
        drifts.iter().all(|line| line.contains("unreadable")),
        "{drifts:?}"
    );
    Ok(())
}

#[test]
fn a_missing_disabled_or_rewritten_ruleset_is_reported() -> TestResult {
    let mut response = live()?;
    response[0]["enforcement"] = serde_json::json!("disabled");
    response[1]["rules"] = serde_json::json!([{ "type": "deletion" }]);
    response[1]["conditions"]["ref_name"]["include"] = serde_json::json!(["refs/tags/app-v*"]);
    let drifts = drift(&response)?;
    assert_eq!(drifts.len(), 3, "{drifts:?}");
    response.as_array_mut().map(|rulesets| rulesets.remove(0));
    assert!(
        drift(&response)?
            .iter()
            .any(|line| line == "release-tags: the ruleset does not exist")
    );
    assert_eq!(drift(&serde_json::json!({}))?.len(), 1);
    Ok(())
}

#[test]
fn the_ruleset_drift_command_parses() -> TestResult {
    let cli = parse_cli(["velnor-gen", "release", "ruleset-drift", "live.json"])?;
    assert_eq!(
        cli.command,
        Cmd::Release(ReleaseCommand::RulesetDrift {
            live: PathBuf::from("live.json")
        })
    );
    assert!(parse_cli(["velnor-gen", "release", "ruleset-drift"]).is_err());
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

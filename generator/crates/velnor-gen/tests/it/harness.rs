// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_gen::io::recorded_files;
use velnor_model::{RepoView, TargetKind};
use velnor_rust::{CONFIG_PATH, DetectError, Reason, detect};

use crate::facts::{TestResult, package, path, repo};
use crate::pipeline::workflow;

const MANIFEST: &str = r#"[package]
name = "a"
[features]
default = ["y"]
x = []
y = ["z"]
z = []
[[bin]]
name = "tool"
harness = false
[[test]]
name = "defaulted"
harness = false
required-features = ["z"]
[[test]]
name = "custom"
harness = false
[[test]]
name = "plain"
[[test]]
name = "gated"
harness = false
required-features = ["x"]
[[test]]
name = "shadow"
harness = false
"#;

const NEXTEST_MISE: &str =
    "[tools]\nrust = \"1.99.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n";

fn view(config: Option<&str>) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut a = package(
        "a",
        "a/Cargo.toml",
        &[],
        &[
            ("custom", TargetKind::Test, true),
            ("plain", TargetKind::Test, true),
            ("gated", TargetKind::Test, true),
            ("shadow", TargetKind::Example, true),
            ("defaulted", TargetKind::Test, true),
            ("tool", TargetKind::Bin, true),
        ],
    )?;
    for target in &mut a.targets {
        target.requires_features = matches!(target.name.as_str(), "gated" | "defaulted");
    }
    let mut view = repo(
        vec![("Cargo.toml", vec![a])],
        &[("a/Cargo.toml", MANIFEST), ("mise.toml", NEXTEST_MISE)],
    )?;
    if let Some(config) = config {
        view.tracked_paths.insert(CONFIG_PATH);
        view.files.insert(CONFIG_PATH, config.as_bytes().to_vec());
    }
    Ok(view)
}

#[test]
fn a_harness_false_test_target_runs_with_cargo_test_unless_the_config_says_it_follows_nextest()
-> TestResult {
    let silent = workflow(&view(None)?)?;
    assert!(silent.contains(
        "cargo test -p a --locked --message-format=json-render-diagnostics --test=custom"
    ));
    assert!(silent.contains(
        "cargo test -p a --locked --message-format=json-render-diagnostics --test=defaulted"
    ));
    assert!(
        silent.contains(
            "cargo test -p a --locked --message-format=json-render-diagnostics --bin=tool"
        )
    );
    for excluded in ["a::custom", "a::defaulted", "a::bin/tool"] {
        assert!(
            silent.contains(&format!("- binary_id(={excluded})")),
            "{excluded}\n{silent}"
        );
    }
    assert!(!silent.contains("binary_id(=a::plain)"));
    assert!(!silent.contains("--test=plain"));
    assert!(!silent.contains("--test=gated"));
    assert!(!silent.contains("--test=shadow"));
    let declared = workflow(&view(Some("nextest_harnesses = [\"a:custom\"]\n"))?)?;
    assert!(!declared.contains("--test=custom"));
    assert!(!declared.contains("binary_id(=a::custom)"));
    Ok(())
}

#[test]
fn a_nextest_harness_that_is_not_a_harness_false_target_is_rejected() -> TestResult {
    for entry in ["a:plain", "a:missing", "b:custom"] {
        let result = detect(&view(Some(&format!(
            "nextest_harnesses = [\"{entry}\"]\n"
        )))?);
        assert_eq!(
            result.err(),
            Some(DetectError::Invalid {
                path: CONFIG_PATH,
                reason: Reason::UnknownNextestHarness(entry.to_owned()),
            }),
            "{entry}"
        );
    }
    Ok(())
}

#[test]
fn a_member_manifest_that_does_not_parse_is_an_error_not_a_package_without_harnesses() -> TestResult
{
    let mut view = view(None)?;
    view.files
        .insert(path("a/Cargo.toml")?, b"[[test\nname = ".to_vec());
    assert!(matches!(
        detect(&view),
        Err(DetectError::Invalid {
            reason: Reason::Syntax(_),
            ..
        })
    ));
    Ok(())
}

#[test]
fn a_member_manifest_git_does_not_track_is_an_error_not_a_package_without_harnesses() -> TestResult
{
    let mut view = view(None)?;
    let manifest = path("a/Cargo.toml")?;
    view.tracked_paths.remove(&manifest);
    view.files.remove(&manifest);
    assert_eq!(
        detect(&view).err(),
        Some(DetectError::Invalid {
            path: manifest,
            reason: Reason::UntrackedManifest,
        })
    );
    Ok(())
}

#[test]
fn a_workspace_with_only_custom_harness_tests_gets_no_nextest_step_and_needs_no_nextest()
-> TestResult {
    let mut only = package(
        "a",
        "a/Cargo.toml",
        &[],
        &[("custom", TargetKind::Test, true)],
    )?;
    only.targets
        .retain(|target| target.name.as_str() == "custom");
    let manifest = "[package]\nname = \"a\"\n[[test]]\nname = \"custom\"\nharness = false\n";
    let view = repo(
        vec![("Cargo.toml", vec![only])],
        &[("a/Cargo.toml", manifest)],
    )?;
    let text = workflow(&view)?;
    assert!(!text.contains("nextest"), "{text}");
    assert!(text.contains("--test=custom"));
    Ok(())
}

#[test]
fn a_nextest_harness_entry_that_two_roots_both_declare_is_rejected() -> TestResult {
    let manifest = "[package]\nname = \"a\"\n[[test]]\nname = \"custom\"\nharness = false\n";
    let one = package(
        "a",
        "a/Cargo.toml",
        &[],
        &[("custom", TargetKind::Test, true)],
    )?;
    let two = package(
        "a",
        "b/a/Cargo.toml",
        &[],
        &[("custom", TargetKind::Test, true)],
    )?;
    let mut view = repo(
        vec![("Cargo.toml", vec![one]), ("b/Cargo.toml", vec![two])],
        &[
            ("a/Cargo.toml", manifest),
            ("b/a/Cargo.toml", manifest),
            ("mise.toml", NEXTEST_MISE),
        ],
    )?;
    view.tracked_paths.insert(CONFIG_PATH);
    view.files.insert(
        CONFIG_PATH,
        b"nextest_harnesses = [\"a:custom\"]\n".to_vec(),
    );
    assert_eq!(
        detect(&view).err(),
        Some(DetectError::Invalid {
            path: CONFIG_PATH,
            reason: Reason::AmbiguousNextestHarness("a:custom".to_owned()),
        })
    );
    Ok(())
}

#[test]
fn a_recorded_sample_keeps_the_member_manifests_detection_reads() -> TestResult {
    let tracked = BTreeSet::from([path("a/Cargo.toml")?, path("a/src/x.rs")?]);
    let files = BTreeMap::from([
        (path("a/Cargo.toml")?, b"[package]".to_vec()),
        (path("a/src/x.rs")?, b"fn main() {}".to_vec()),
    ]);
    let recorded = recorded_files(files, &tracked);
    assert_eq!(
        recorded.keys().collect::<Vec<_>>(),
        [&path("a/Cargo.toml")?]
    );
    Ok(())
}

#[test]
fn a_feature_lane_excludes_the_harness_binaries_that_nextest_cannot_run() -> TestResult {
    let lane = "[[test_lane]]\nroot = '.'\nfeatures = ['x']\nfilter = 'test(~a)'\n";
    let text = workflow(&view(Some(lane))?)?;
    let step = text
        .lines()
        .find(|line| line.contains("--features ''x''"))
        .ok_or("no feature lane step")?;
    for excluded in ["a::custom", "a::defaulted", "a::bin/tool"] {
        assert!(
            step.contains(&format!("- binary_id(={excluded})")),
            "{excluded}\n{step}"
        );
    }
    assert!(
        step.contains("-E \"(($TEST_FILTER) and (test(~a)))"),
        "{step}"
    );
    Ok(())
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{RepoView, TargetKind};
use velnor_rust::{CONFIG_PATH, DetectError, Reason, detect};

use crate::facts::{TestResult, package, path, repo};
use crate::pipeline::workflow;

const MANIFEST: &str = r#"[package]
name = "a"
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
        ],
    )?;
    for target in &mut a.targets {
        target.requires_features = target.name.as_str() == "gated";
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
    assert!(silent.contains("cargo test -p a --test custom --locked"));
    assert!(!silent.contains("--test plain"));
    assert!(!silent.contains("--test gated"));
    assert!(!silent.contains("--test shadow"));
    let declared = workflow(&view(Some("nextest_harnesses = [\"a:custom\"]\n"))?)?;
    assert!(!declared.contains("--test custom"));
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

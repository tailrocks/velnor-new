// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{DependencyKind, RepoView, TargetKind};

use crate::facts::{TestResult, package, repo};
use crate::pipeline::workflow;

const NEXTEST_MISE: &str = "[tools]\nrust = { version = \"1.99.1\", mr_boxington = true }\nmr-boxington = \"v1.30.0\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n\n[wrappers.cargo]\ncommand = \"mbx\"\nenv = { MBX_CARGO_SHIM_MODE = \"1\" }\n";

fn view(
    manifests: &[(&str, &str)],
    packages: Vec<velnor_model::RustPackage>,
) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut files = vec![("mise.toml", NEXTEST_MISE)];
    files.extend_from_slice(manifests);
    repo(vec![("Cargo.toml", packages)], &files)
}

#[test]
fn a_dependency_feature_in_required_features_runs_the_target_in_a_direct_step() -> TestResult {
    let manifest = "[package]\nname = \"a\"\n[dependencies]\nfoo = { version = \"1\" }\n[[test]]\nname = \"custom\"\nharness = false\nrequired-features = [\"foo/bar\"]\n";
    let a = package(
        "a",
        "a/Cargo.toml",
        &[],
        &[("custom", TargetKind::Test, true)],
    )?;
    let text = workflow(&view(&[("a/Cargo.toml", manifest)], vec![a])?)?;
    assert!(
        text.contains("cargo test -p a --locked --message-format=json-render-diagnostics --test=custom --features ''foo/bar''"),
        "{text}"
    );
    assert!(!text.contains("nextest"), "{text}");
    Ok(())
}

#[test]
fn a_cdylib_library_with_a_custom_harness_runs_with_cargo_test_lib() -> TestResult {
    let manifest = "[package]\nname = \"a\"\n[lib]\ncrate-type = [\"cdylib\"]\nharness = false\n";
    let a = package("a", "a/Cargo.toml", &[], &[("a", TargetKind::Lib, true)])?;
    let text = workflow(&view(&[("a/Cargo.toml", manifest)], vec![a])?)?;
    assert!(
        text.contains("cargo test -p a --locked --message-format=json-render-diagnostics --lib"),
        "{text}"
    );
    assert!(!text.contains("nextest"), "{text}");
    Ok(())
}

#[test]
fn a_weak_feature_forward_from_a_sibling_enables_the_gated_harness() -> TestResult {
    let gated = "[package]\nname = \"a\"\n[features]\nx = []\n[[test]]\nname = \"gated\"\nharness = false\nrequired-features = [\"x\"]\n";
    let weak = "[package]\nname = \"b\"\n[features]\ndefault = [\"a?/x\"]\n";
    let a = package(
        "a",
        "a/Cargo.toml",
        &[],
        &[("gated", TargetKind::Test, true)],
    )?;
    let b = package(
        "b",
        "b/Cargo.toml",
        &[("a", DependencyKind::Normal, "a/Cargo.toml")],
        &[("b", TargetKind::Lib, true)],
    )?;
    let text = workflow(&view(
        &[("a/Cargo.toml", gated), ("b/Cargo.toml", weak)],
        vec![a, b],
    )?)?;
    assert!(text.contains("--test=gated"), "{text}");
    Ok(())
}

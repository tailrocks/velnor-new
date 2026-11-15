// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{DependencyKind, RepoView, TargetKind};
use velnor_rust::{CONFIG_PATH, DetectError, Reason, detect};

use crate::facts::{TestResult, package, repo};
use crate::pipeline::workflow;

const NEXTEST_MISE: &str =
    "[tools]\nrust = \"1.99.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n";

const GATED: &str = "[package]\nname = \"a\"\n[features]\nx = []\n[[test]]\nname = \"gated\"\nharness = false\nrequired-features = [\"x\"]\n";

fn view(
    manifests: &[(&str, &str)],
    packages: Vec<velnor_model::RustPackage>,
) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut files = vec![("mise.toml", NEXTEST_MISE)];
    files.extend_from_slice(manifests);
    repo(vec![("Cargo.toml", packages)], &files)
}

#[test]
fn a_feature_another_member_requests_on_the_package_enables_its_gated_harness() -> TestResult {
    let a = package(
        "a",
        "a/Cargo.toml",
        &[],
        &[("gated", TargetKind::Test, true)],
    )?;
    let mut b = package(
        "b",
        "b/Cargo.toml",
        &[("a", DependencyKind::Normal, "a/Cargo.toml")],
        &[("b", TargetKind::Lib, true)],
    )?;
    let alone = view(
        &[
            ("a/Cargo.toml", GATED),
            ("b/Cargo.toml", "[package]\nname = \"b\"\n"),
        ],
        vec![a.clone(), b.clone()],
    )?;
    assert!(!workflow(&alone)?.contains("--test=gated"));
    b.dependencies[0].features = vec!["x".to_owned()];
    let unified = view(
        &[
            ("a/Cargo.toml", GATED),
            ("b/Cargo.toml", "[package]\nname = \"b\"\n"),
        ],
        vec![a, b],
    )?;
    assert!(workflow(&unified)?.contains("--test=gated"));
    Ok(())
}

#[test]
fn a_bin_and_a_test_with_one_name_make_an_nextest_override_ambiguous() -> TestResult {
    let manifest = "[package]\nname = \"a\"\n[[bin]]\nname = \"dup\"\nharness = false\n[[test]]\nname = \"dup\"\nharness = false\n";
    let a = package(
        "a",
        "a/Cargo.toml",
        &[],
        &[
            ("dup", TargetKind::Bin, true),
            ("dup", TargetKind::Test, true),
        ],
    )?;
    let mut view = view(&[("a/Cargo.toml", manifest)], vec![a])?;
    view.tracked_paths.insert(CONFIG_PATH);
    view.files
        .insert(CONFIG_PATH, b"nextest_harnesses = [\"a:dup\"]\n".to_vec());
    assert_eq!(
        detect(&view).err(),
        Some(DetectError::Invalid {
            path: CONFIG_PATH,
            reason: Reason::AmbiguousNextestHarness("a:dup".to_owned()),
        })
    );
    Ok(())
}

#[test]
fn members_sharing_a_harness_target_name_get_one_direct_step() -> TestResult {
    let manifest = |name: &str| {
        format!("[package]\nname = \"{name}\"\n[[test]]\nname = \"custom\"\nharness = false\n")
    };
    let a = package(
        "a",
        "a/Cargo.toml",
        &[],
        &[("custom", TargetKind::Test, true)],
    )?;
    let b = package(
        "b",
        "b/Cargo.toml",
        &[],
        &[("custom", TargetKind::Test, true)],
    )?;
    let (ma, mb) = (manifest("a"), manifest("b"));
    let text = workflow(&view(
        &[("a/Cargo.toml", &ma), ("b/Cargo.toml", &mb)],
        vec![a, b],
    )?)?;
    assert_eq!(text.matches("--test=custom").count(), 1, "{text}");
    Ok(())
}

#[test]
fn a_proc_macro_library_with_a_custom_harness_runs_with_cargo_test_lib() -> TestResult {
    let manifest = "[package]\nname = \"a\"\n[lib]\nproc-macro = true\nharness = false\n";
    let a = package(
        "a",
        "a/Cargo.toml",
        &[],
        &[("a", TargetKind::ProcMacro, true)],
    )?;
    let text = workflow(&view(&[("a/Cargo.toml", manifest)], vec![a])?)?;
    assert!(
        text.contains("cargo test --workspace --locked --lib"),
        "{text}"
    );
    assert!(!text.contains("nextest"), "{text}");
    Ok(())
}

#[test]
fn a_dependency_feature_enables_a_local_feature_only_through_an_implicit_optional_dependency_feature()
-> TestResult {
    let manifest = |dependency: &str, local: &str| {
        format!(
            "[package]\nname = \"a\"\n[features]\ndefault = [\"foo/bar\"]\n{local}[dependencies]\nfoo = {dependency}\n[[test]]\nname = \"custom\"\nharness = false\nrequired-features = [\"foo\"]\n"
        )
    };
    let render = |dependency: &str, local: &str| -> Result<String, Box<dyn std::error::Error>> {
        let a = package(
            "a",
            "a/Cargo.toml",
            &[],
            &[("custom", TargetKind::Test, true)],
        )?;
        workflow(&view(
            &[("a/Cargo.toml", &manifest(dependency, local))],
            vec![a],
        )?)
    };
    assert!(!render("{ version = \"1\" }", "foo = []\n")?.contains("--test=custom"));
    assert!(render("{ version = \"1\", optional = true }", "")?.contains("--test=custom"));
    Ok(())
}

#[test]
fn a_feature_a_sibling_forwards_counts_and_another_workspaces_request_does_not() -> TestResult {
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
    let forwarding = "[package]\nname = \"b\"\n[features]\ndefault = [\"a/x\"]\n";
    let forwarded = view(
        &[("a/Cargo.toml", GATED), ("b/Cargo.toml", forwarding)],
        vec![a.clone(), b],
    )?;
    assert!(workflow(&forwarded)?.contains("--test=gated"));

    let mut other = package(
        "c",
        "c/Cargo.toml",
        &[("a", DependencyKind::Normal, "a/Cargo.toml")],
        &[("c", TargetKind::Lib, true)],
    )?;
    other.dependencies[0].features = vec!["x".to_owned()];
    let separate = repo(
        vec![("Cargo.toml", vec![a]), ("c/Cargo.toml", vec![other])],
        &[
            ("mise.toml", NEXTEST_MISE),
            ("a/Cargo.toml", GATED),
            ("c/Cargo.toml", "[package]\nname = \"c\"\n"),
        ],
    )?;
    assert!(!workflow(&separate)?.contains("--test=gated"));
    Ok(())
}

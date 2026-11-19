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
fn members_sharing_a_harness_target_name_each_get_a_direct_step() -> TestResult {
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
    assert!(
        text.contains(
            "cargo test -p a --locked --message-format=json-render-diagnostics --test=custom"
        ),
        "{text}"
    );
    assert!(
        text.contains(
            "cargo test -p b --locked --message-format=json-render-diagnostics --test=custom"
        ),
        "{text}"
    );
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
        text.contains("cargo test -p a --locked --message-format=json-render-diagnostics --lib"),
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

fn forwarding_view(
    forwarder: &str,
    kind: DependencyKind,
    features: &[&str],
    workspace: &str,
) -> Result<RepoView, Box<dyn std::error::Error>> {
    let a = package(
        "a",
        "a/Cargo.toml",
        &[],
        &[("gated", TargetKind::Test, true)],
    )?;
    let mut b = package(
        "b",
        "b/Cargo.toml",
        &[("a", kind, "a/Cargo.toml")],
        &[("b", TargetKind::Lib, true)],
    )?;
    b.dependencies[0].features = features.iter().map(|f| (*f).to_owned()).collect();
    view(
        &[
            ("Cargo.toml", workspace),
            ("a/Cargo.toml", GATED),
            ("b/Cargo.toml", forwarder),
        ],
        vec![a, b],
    )
}

const WORKSPACE: &str = "[workspace]\nmembers = [\"a\", \"b\"]\nresolver = \"2\"\n";

#[test]
fn a_feature_forwarded_through_a_renamed_dependency_enables_the_gated_harness() -> TestResult {
    let renamed = "[package]\nname = \"b\"\n[features]\ndefault = [\"aa/x\"]\n[dependencies]\naa = { package = \"a\", path = \"../a\" }\n";
    let text = workflow(&forwarding_view(
        renamed,
        DependencyKind::Normal,
        &[],
        WORKSPACE,
    )?)?;
    assert!(text.contains("--test=gated"), "{text}");
    Ok(())
}

#[test]
fn a_feature_forwarded_only_by_a_disabled_sibling_feature_does_not_enable_the_gated_harness()
-> TestResult {
    let dormant = "[package]\nname = \"b\"\n[features]\ndormant = [\"a/x\"]\n";
    let text = workflow(&forwarding_view(
        dormant,
        DependencyKind::Normal,
        &[],
        WORKSPACE,
    )?)?;
    assert!(!text.contains("--test=gated"), "{text}");
    Ok(())
}

#[test]
fn a_build_dependency_feature_reaches_the_tests_only_when_the_resolver_unifies_it() -> TestResult {
    let manifest = "[package]\nname = \"b\"\n[build-dependencies]\na = { path = \"../a\" }\n";
    let split = workflow(&forwarding_view(
        manifest,
        DependencyKind::Build,
        &["x"],
        WORKSPACE,
    )?)?;
    assert!(!split.contains("--test=gated"), "{split}");
    let unified = workflow(&forwarding_view(
        manifest,
        DependencyKind::Build,
        &["x"],
        "[workspace]\nmembers = [\"a\", \"b\"]\nresolver = \"1\"\n",
    )?)?;
    assert!(unified.contains("--test=gated"), "{unified}");
    Ok(())
}

#[test]
fn a_package_scoped_harness_carries_the_features_it_requires() -> TestResult {
    let forwarding = forwarding_view(
        "[package]\nname = \"b\"\n",
        DependencyKind::Normal,
        &["x"],
        WORKSPACE,
    )?;
    let text = workflow(&forwarding)?;
    assert!(
        text.contains("cargo test -p a --locked --message-format=json-render-diagnostics --test=gated --features ''x''"),
        "{text}"
    );
    Ok(())
}

#[test]
fn a_feature_forwarded_through_a_workspace_inherited_alias_enables_the_gated_harness() -> TestResult
{
    let inherited = "[package]\nname = \"b\"\n[features]\ndefault = [\"aa/x\"]\n[dependencies]\naa.workspace = true\n";
    let mut view = forwarding_view(inherited, DependencyKind::Normal, &[], WORKSPACE)?;
    view.rust_metadata
        .values_mut()
        .flat_map(|metadata| metadata.members.iter_mut())
        .filter(|member| member.name.as_str() == "b")
        .for_each(|member| member.dependencies[0].rename = Some("aa".to_owned()));
    let text = workflow(&view)?;
    assert!(text.contains("--test=gated"), "{text}");
    Ok(())
}

#[test]
fn the_per_package_steps_and_the_archive_build_follow_the_packages_the_plan_selected() -> TestResult
{
    let manifest = "[package]\nname = \"a\"\n[[test]]\nname = \"custom\"\nharness = false\n";
    let a = package(
        "a",
        "a/Cargo.toml",
        &[],
        &[
            ("a", TargetKind::Lib, true),
            ("custom", TargetKind::Test, true),
        ],
    )?;
    let b = package("b", "b/Cargo.toml", &[], &[("b", TargetKind::Lib, true)])?;
    let config = "shape = \"groups\"\n";
    let text = workflow(&repo(
        vec![("Cargo.toml", vec![a, b])],
        &[
            ("mise.toml", NEXTEST_MISE),
            ("a/Cargo.toml", manifest),
            (CONFIG_PATH.as_str(), config),
        ],
    )?)?
    .replace("''", "'");
    let archive = text
        .lines()
        .find(|line| line.contains("nextest archive"))
        .unwrap_or_default();
    assert!(
        archive.contains("archive --workspace --locked") && !archive.contains("PACKAGE_ARGS"),
        "{archive}"
    );
    for (name, package) in [
        ("Test a custom", "a"),
        ("Check a alone", "a"),
        ("Check b alone", "b"),
    ] {
        let marker = format!("name: {name}\n");
        let block: String = text
            .lines()
            .skip_while(|line| !line.ends_with(marker.trim_end()))
            .take_while(|line| !line.contains("run: "))
            .collect::<Vec<_>>()
            .join("\n");
        let selected = format!(
            "contains(format(' {{0}} ', (steps.replan.outputs.rust_root_package_args || needs.rust-plan.outputs.rust_root_package_args)), ' -p {package} ')"
        );
        assert!(block.contains(&selected), "{name}: {block}");
        assert!(block.contains("' --workspace '"), "{name}: {block}");
    }
    Ok(())
}

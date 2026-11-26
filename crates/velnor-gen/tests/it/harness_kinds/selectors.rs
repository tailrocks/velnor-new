// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{DependencyKind, TargetKind};
use velnor_rust::CONFIG_PATH;

use super::{GATED, WORKSPACE, view};
use crate::facts::{TestResult, package};
use crate::pipeline::{pin_helper, workflow};

const GATED_ON_Y: &str = "[package]\nname = \"a\"\n[features]\nx = []\ny = []\n[[test]]\nname = \"gated\"\nharness = false\nrequired-features = [\"y\"]\n";

fn workspace_selector(
    forwarder: &str,
    gated: &str,
    config: &str,
    aliases: &[(&str, bool, &[&str])],
) -> Result<bool, Box<dyn std::error::Error>> {
    let a = package(
        "a",
        "a/Cargo.toml",
        &[],
        &[("gated", TargetKind::Test, true)],
    )?;
    let mut b = package("b", "b/Cargo.toml", &[], &[("b", TargetKind::Lib, true)])?;
    for (rename, optional, features) in aliases {
        let mut edge = package(
            "b",
            "b/Cargo.toml",
            &[("a", DependencyKind::Normal, "a/Cargo.toml")],
            &[],
        )?
        .dependencies
        .remove(0);
        edge.rename = Some((*rename).to_owned());
        edge.optional = *optional;
        edge.features = features.iter().map(|f| (*f).to_owned()).collect();
        b.dependencies.push(edge);
    }
    let mut repository = view(
        &[
            ("Cargo.toml", WORKSPACE),
            ("a/Cargo.toml", gated),
            ("b/Cargo.toml", forwarder),
        ],
        vec![a, b],
    )?;
    repository.tracked_paths.insert(CONFIG_PATH);
    repository
        .files
        .insert(CONFIG_PATH, config.as_bytes().to_vec());
    pin_helper(&mut repository, "0.1.117")?;
    Ok(workflow(&repository)?.contains("--test=gated"))
}

const OPTIONAL_ALIAS: &str = "alias = { package = \"a\", path = \"../a\", optional = true }\n";

#[test]
fn a_workspace_selector_enables_the_member_behind_a_renamed_optional_dependency() -> TestResult {
    let plain = format!("[package]\nname = \"b\"\n[dependencies]\n{OPTIONAL_ALIAS}");
    let alias = [("alias", true, &[][..])];
    assert!(workspace_selector(
        &plain,
        GATED,
        "workspace_features = ['alias/x']\n",
        &alias
    )?);
    let suppressed = format!(
        "[package]\nname = \"b\"\n[features]\nsuppress = [\"dep:alias\"]\n[dependencies]\n{OPTIONAL_ALIAS}"
    );
    assert!(workspace_selector(
        &suppressed,
        GATED,
        "workspace_features = ['alias/x']\n",
        &alias
    )?);
    let ignored = "workspace_features = ['local-mode']\n";
    let with_feature = format!(
        "[package]\nname = \"b\"\n[features]\nlocal-mode = []\n[dependencies]\n{OPTIONAL_ALIAS}"
    );
    assert!(!workspace_selector(&with_feature, GATED, ignored, &alias)?);
    Ok(())
}

#[test]
fn a_weak_dependency_selector_enables_the_member_only_once_the_dependency_is_active() -> TestResult
{
    let forwarder = |selector: &str| {
        format!(
            "[package]\nname = \"b\"\n[features]\nci = [\"{selector}\"]\n[dependencies]\n{OPTIONAL_ALIAS}"
        )
    };
    let alias = [("alias", true, &[][..])];
    let config = "workspace_features = ['ci']\n";
    assert!(workspace_selector(
        &forwarder("alias/x"),
        GATED,
        config,
        &alias
    )?);
    assert!(!workspace_selector(
        &forwarder("alias?/x"),
        GATED,
        config,
        &alias
    )?);
    Ok(())
}

#[test]
fn an_inactive_optional_dependency_and_a_second_alias_forward_nothing() -> TestResult {
    let optional = format!(
        "[package]\nname = \"b\"\n[features]\nother = []\n[dependencies]\n{OPTIONAL_ALIAS}"
    );
    let required = "[package]\nname = \"b\"\n[features]\nother = []\n[dependencies]\nalias = { package = \"a\", path = \"../a\" }\n";
    let config = "workspace_features = ['other']\n";
    assert!(!workspace_selector(
        &optional,
        GATED,
        config,
        &[("alias", true, &["x"][..])]
    )?);
    assert!(workspace_selector(
        required,
        GATED,
        config,
        &[("alias", false, &["x"][..])]
    )?);
    let two = format!(
        "[package]\nname = \"b\"\n[dependencies]\n{OPTIONAL_ALIAS}second = {{ package = \"a\", path = \"../a\", optional = true, features = [\"y\"] }}\n"
    );
    assert!(!workspace_selector(
        &two,
        GATED_ON_Y,
        "workspace_features = ['alias/x']\n",
        &[("alias", true, &[][..]), ("second", true, &["y"][..])]
    )?);
    Ok(())
}

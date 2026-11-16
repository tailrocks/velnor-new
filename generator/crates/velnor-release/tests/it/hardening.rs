// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::{ReleaseError, ReleasePlan, ruleset_json};

use crate::fixture::{MISE_TOML, PLZ, TestResult, edit, files, plz_edited, refusal};

fn plz_refusal(from: &str, to: &str) -> Result<Option<ReleaseError>, String> {
    Ok(refusal(&plz_edited(from, to)?))
}

fn rulesets(
    files: &velnor_release::ReleaseFiles,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let lines = ruleset_json(&ReleasePlan::read(files)?);
    Ok(lines
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<Vec<serde_json::Value>, _>>()?
        .into())
}

fn ruleset(
    files: &velnor_release::ReleaseFiles,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    Ok(rulesets(files)?[0].clone())
}

#[test]
fn only_the_creation_rules_are_bypassed_by_the_app_and_tags_cannot_be_moved_or_deleted()
-> TestResult {
    let trees = rulesets(&files())?;
    assert_eq!(trees[0]["rules"], serde_json::json!([{"type": "creation"}]));
    assert_eq!(trees[0]["bypass_actors"][0]["actor_id"], 5_762_496);
    assert_eq!(
        trees[1]["rules"],
        serde_json::json!([{"type": "update"}, {"type": "deletion"}])
    );
    assert_eq!(trees[1]["bypass_actors"], serde_json::json!([]));
    assert_eq!(trees[0]["conditions"], trees[1]["conditions"]);
    assert_ne!(trees[0]["name"], trees[1]["name"]);
    Ok(())
}

#[test]
fn the_ruleset_protects_the_tags_of_packages_that_inherit_the_workspace_settings() -> TestResult {
    let only_workspace = PLZ.replace("[[package]]\nname = \"app\"\n", "");
    let mut files = files();
    files
        .release_plz_toml
        .insert(".".to_owned(), only_workspace);
    assert_eq!(
        ruleset(&files)?["conditions"]["ref_name"]["include"],
        serde_json::json!(["refs/tags/*-v*"])
    );
    let listed = ruleset(&crate::fixture::files())?;
    assert_eq!(
        listed["conditions"]["ref_name"]["include"],
        serde_json::json!(["refs/tags/*-v*", "refs/tags/app-v*"])
    );
    Ok(())
}

#[test]
fn settings_every_unlisted_package_inherits_are_checked_on_the_workspace() -> TestResult {
    let error = plz_refusal("semver_check = false\n", "semver_check = true\n")?;
    assert!(
        matches!(
            error,
            Some(ReleaseError::SemverCheckNotFalse { package: Some(ref package), .. }) if package == "app"
        ),
        "{error:?}"
    );
    let listed = edit(
        PLZ,
        "name = \"app\"",
        "name = \"app\"\nsemver_check = false",
    )?;
    let mut files = files();
    files.release_plz_toml.insert(
        ".".to_owned(),
        edit(
            &listed,
            "semver_check = false\npublish_no_verify",
            "publish_no_verify",
        )?,
    );
    assert!(matches!(
        refusal(&files),
        Some(ReleaseError::SemverCheckNotFalse { package: None, .. })
    ));
    Ok(())
}

#[test]
fn a_package_that_turns_release_always_back_on_is_refused() -> TestResult {
    let error = plz_refusal("name = \"app\"", "name = \"app\"\nrelease_always = true")?;
    assert!(
        matches!(error, Some(ReleaseError::ReleaseAlwaysNotFalse { .. })),
        "{error:?}"
    );
    Ok(())
}

#[test]
fn disabling_the_tag_or_the_github_release_is_refused() -> TestResult {
    for (key, at_package) in [
        ("git_tag_enable", false),
        ("git_tag_enable", true),
        ("git_release_enable", false),
        ("git_release_enable", true),
    ] {
        let (from, to) = if at_package {
            (
                "name = \"app\"".to_owned(),
                format!("name = \"app\"\n{key} = false"),
            )
        } else {
            (
                "[workspace]\n".to_owned(),
                format!("[workspace]\n{key} = false\n"),
            )
        };
        let error = plz_refusal(&from, &to)?;
        let named = matches!(
            (&error, key),
            (Some(ReleaseError::ReleaseSwitchOff { field, .. }), key) if *field == key
        );
        assert!(named, "{key} {at_package}: {error:?}");
    }
    Ok(())
}

#[test]
fn an_empty_or_blank_release_branch_prefix_is_refused() -> TestResult {
    for prefix in ["", "  "] {
        let to = format!("[workspace]\npr_branch_prefix = \"{prefix}\"\n");
        let error = plz_refusal("[workspace]\n", &to)?;
        assert!(
            matches!(error, Some(ReleaseError::EmptyPrBranchPrefix { .. })),
            "{prefix:?}: {error:?}"
        );
    }
    Ok(())
}

#[test]
fn changelog_commands_and_external_changelog_configs_are_refused() {
    let commands = [
        "[changelog]\ncommit_preprocessors = [{ pattern = \"a\", replace_command = \"curl x\" }]\n",
        "[changelog]\npostprocessors = [{ pattern = \"a\", replace_command = \"sh -c x\" }]\n",
        "[[package]]\nname = \"other\"\nchangelog_config = \"cliff.toml\"\n",
    ];
    for extra in commands {
        let mut files = files();
        files
            .release_plz_toml
            .insert(".".to_owned(), format!("{PLZ}\n{extra}"));
        let error = refusal(&files);
        assert!(
            matches!(error, Some(ReleaseError::ExecutableConfig { .. })),
            "{extra}: {error:?}"
        );
    }
}

#[test]
fn an_executable_cargo_credential_provider_is_refused_for_every_root_whatever_it_publishes()
-> TestResult {
    let git_only = edit(PLZ, "publish_no_verify = true", "git_only = true")?;
    for publishing in [PLZ.to_owned(), git_only] {
        for config in [
            "[registry]\ncredential-provider = [\"./steal\"]\n",
            "[registries.evil]\nindex = \"sparse+http://x\"\ncredential-provider = [\"./steal\"]\n",
            "[credential-alias]\nx = \"./steal\"\n",
            "[registry]\nglobal-credential-providers = [\"./steal\"]\n",
        ] {
            let mut files = files();
            files
                .release_plz_toml
                .insert(".".to_owned(), publishing.clone());
            files
                .cargo_configs
                .insert(".cargo/config.toml".to_owned(), config.to_owned());
            let got = refusal(&files);
            assert!(
                matches!(got, Some(ReleaseError::CargoCredentialProvider { .. })),
                "{config}: {got:?}"
            );
        }
        let mut files = files();
        files.release_plz_toml.insert(".".to_owned(), publishing);
        files.cargo_configs.insert(
            ".cargo/config.toml".to_owned(),
            "[registry]\ncredential-provider = \"cargo:token\"\n".to_owned(),
        );
        assert_eq!(refusal(&files), None);
    }
    Ok(())
}

#[test]
fn a_toolchain_taken_from_a_path_is_refused_for_every_root() -> TestResult {
    let git_only = edit(PLZ, "publish_no_verify = true", "git_only = true")?;
    for publishing in [PLZ.to_owned(), git_only] {
        for (file, text) in [
            ("rust-toolchain.toml", "[toolchain]\npath = \"/work/tc\"\n"),
            ("rust-toolchain", "[toolchain]\npath = \"tc\"\n"),
            ("rust-toolchain", "/work/tc\n"),
        ] {
            let mut files = files();
            files
                .release_plz_toml
                .insert(".".to_owned(), publishing.clone());
            files
                .toolchain_files
                .insert(file.to_owned(), text.to_owned());
            let got = refusal(&files);
            assert!(
                matches!(got, Some(ReleaseError::ToolchainFromPath { .. })),
                "{file} {text}: {got:?}"
            );
        }
    }
    for (file, text) in [
        ("rust-toolchain.toml", "[toolchain]\nchannel = \"1.99.1\"\n"),
        ("rust-toolchain", "stable\n"),
    ] {
        let mut files = files();
        files
            .toolchain_files
            .insert(file.to_owned(), text.to_owned());
        assert_eq!(refusal(&files), None, "{file}");
    }
    Ok(())
}

#[test]
fn a_repository_that_needs_nightly_rust_is_refused() {
    for (file, text) in [
        (
            "rust-toolchain.toml",
            "[toolchain]\nchannel = \"nightly\"\n",
        ),
        (
            "rust-toolchain.toml",
            "[toolchain]\nchannel = \"nightly-2026-01-01\"\n",
        ),
        ("rust-toolchain", "nightly\n"),
    ] {
        let mut files = files();
        files
            .toolchain_files
            .insert(file.to_owned(), text.to_owned());
        assert!(
            matches!(refusal(&files), Some(ReleaseError::NightlyOnly { .. })),
            "{file} {text}"
        );
    }
    let mut features = files();
    features.cargo_manifest_texts.insert(
        "Cargo.toml".to_owned(),
        "cargo-features = [\"edition2024\"]\n[package]\nname = \"app\"\n".to_owned(),
    );
    assert!(matches!(
        refusal(&features),
        Some(ReleaseError::NightlyOnly { .. })
    ));
}

#[test]
fn only_a_toolchain_file_rustup_finds_from_the_checkout_root_is_checked() {
    let mut nested = files();
    nested.toolchain_files.insert(
        "tools/rust-toolchain.toml".to_owned(),
        "[toolchain]\npath = \"/work/tc\"\n".to_owned(),
    );
    assert_eq!(refusal(&nested), None);
    nested.toolchain_files.insert(
        "rust-toolchain.toml".to_owned(),
        "[toolchain]\npath = \"/work/tc\"\n".to_owned(),
    );
    assert!(matches!(
        refusal(&nested),
        Some(ReleaseError::ToolchainFromPath { file }) if file == "rust-toolchain.toml"
    ));
}

#[test]
fn a_root_written_with_a_dot_or_an_empty_component_is_refused() {
    for root in ["./tools", "tools/", "tools//sub", "tools/./sub", ""] {
        let mut files = files();
        files.release_toml = Some(format!("roots = [{root:?}]\n"));
        assert!(
            matches!(refusal(&files), Some(ReleaseError::UnsafeRoot { .. })),
            "{root:?}"
        );
    }
}

#[test]
fn tag_patterns_that_can_render_the_same_tag_are_refused() -> TestResult {
    let text = edit(
        PLZ,
        "name = \"app\"",
        "name = \"app\"\ngit_tag_name = \"v{{ version }}\"\n\n[[package]]\nname = \"tool\"\ngit_tag_name = \"v1{{ version }}\"",
    )?;
    let mut files = files();
    files.release_plz_toml.insert(".".to_owned(), text);
    assert!(matches!(
        refusal(&files),
        Some(ReleaseError::TagsMayCollide { .. })
    ));
    let apart = edit(
        PLZ,
        "name = \"app\"",
        "name = \"app\"\n\n[[package]]\nname = \"app-cli\"",
    )?;
    files.release_plz_toml.insert(".".to_owned(), apart);
    assert_eq!(refusal(&files), None);
    Ok(())
}

#[test]
fn release_plz_may_sit_in_either_mise_file() {
    let mut files = files();
    files.mise_tomls = vec![
        "[tools]\nbun = \"1.4.4\"\n".to_owned(),
        MISE_TOML.to_owned(),
    ];
    assert_eq!(refusal(&files), None);
    files.mise_tomls = vec![
        MISE_TOML.to_owned(),
        "[tools]\nbun = \"1.4.4\"\n".to_owned(),
    ];
    assert_eq!(refusal(&files), None);
}

#[test]
fn only_the_cargo_token_built_in_passes_as_a_credential_provider() {
    for provider in [
        "\"cargo:token\"",
        "[\"cargo:token\"]",
        "[\"cargo:token\", \"cargo:token\"]",
    ] {
        let mut files = files();
        files.cargo_configs.insert(
            ".cargo/config.toml".to_owned(),
            format!("[registry]\ncredential-provider = {provider}\n"),
        );
        assert_eq!(refusal(&files), None, "{provider}");
    }
    for provider in [
        "\"cargo:wincred\"",
        "\"cargo:macos-keychain\"",
        "\"cargo:libsecret\"",
        "\"cargo:token-from-stdout cat\"",
        "\"cargo:token-from-stdout ./steal\"",
        "\"cargo:\"",
        "\"cargo:token \"",
        "\"Cargo:token\"",
        "[\"cargo:token\", \"cargo:wincred\"]",
        "[\"cargo:token\", \"./steal\"]",
        "[]",
        "true",
    ] {
        let mut files = files();
        files.cargo_configs.insert(
            ".cargo/config.toml".to_owned(),
            format!("[registry]\ncredential-provider = {provider}\n"),
        );
        let got = refusal(&files);
        assert!(
            matches!(got, Some(ReleaseError::CargoCredentialProvider { .. })),
            "{provider}: {got:?}"
        );
    }
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_release::{ReleaseError, ReleaseFiles, ReleasePlan};

use crate::fixture::{PLZ, TestResult, edit, files, locked, refusal, with_metadata};

fn with_mise(text: &str) -> ReleaseFiles {
    let mut files = files();
    files.mise_tomls = vec![text.to_owned()];
    files
}

fn names(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

pub(crate) fn two_roots(tools_plz: &str) -> Result<ReleaseFiles, String> {
    let mut files = files();
    files.release_toml = Some("roots = [\".\", \"tools\"]\n".to_owned());
    let root = edit(
        PLZ,
        "[workspace]\n",
        "[workspace]\npr_branch_prefix = \"app-\"\n",
    )?;
    files.release_plz_toml.insert(".".to_owned(), root);
    files
        .release_plz_toml
        .insert("tools".to_owned(), tools_plz.to_owned());
    Ok(files)
}

#[test]
fn release_plz_must_be_pinned_to_an_exact_version_that_mise_cannot_redirect() {
    for version in [
        "path:/tmp/evil",
        "latest",
        "0.4",
        "^0.4.3",
        "0.4.3-rc1",
        "ref:main",
    ] {
        let text = format!("[tools]\nrelease-plz = \"{version}\"\n");
        let mut files = with_mise(&text);
        files.mise_lock = format!("[[tools.release-plz]]\nversion = \"{version}\"\n");
        assert!(
            matches!(
                refusal(&files),
                Some(ReleaseError::ReleasePlzVersionNotExact { .. })
            ),
            "{version}"
        );
    }
}

#[test]
fn the_plan_installs_the_exact_configured_version_whatever_key_or_options_mise_has() -> TestResult {
    assert_eq!(ReleasePlan::read(&files())?.release_plz_version, "0.4.3");
    let mut files =
        with_mise("[tools]\n\"github:release-plz/release-plz\" = { version = \"0.4.3\" }\n");
    files.mise_lock = locked("github:release-plz/release-plz", "0.4.3");
    assert_eq!(ReleasePlan::read(&files)?.release_plz_version, "0.4.3");
    Ok(())
}

#[test]
fn the_hidden_mise_file_wins_over_mise_toml_for_the_same_selector() {
    let stale = "[tools]\nrelease-plz = \"0.3.0\"\n".to_owned();
    let current = "[tools]\nrelease-plz = \"0.4.3\"\n".to_owned();
    let mut files = files();
    files.mise_tomls = vec![stale.clone(), current.clone()];
    assert_eq!(refusal(&files), None);
    files.mise_tomls = vec![current, stale];
    assert!(matches!(
        refusal(&files),
        Some(ReleaseError::ReleasePlzLockMismatch { .. })
    ));
}

fn workspace_only(prefix: &str, tag: &str) -> String {
    format!(
        "[workspace]\ngit_tag_name = \"{tag}\"\ngit_release_draft = true\nrelease_always = false\nsemver_check = false\npublish_no_verify = true\npr_branch_prefix = \"{prefix}\"\n"
    )
}

#[test]
fn workspace_wide_tag_patterns_of_two_roots_that_can_collide_are_refused() -> TestResult {
    let mut files = two_roots(PLZ_TOOLS)?;
    for (root, prefix) in [(".", "app-"), ("tools", "tools-")] {
        files
            .release_plz_toml
            .insert(root.to_owned(), workspace_only(prefix, "v{{ version }}"));
    }
    let error = refusal(&files);
    assert!(
        matches!(error, Some(ReleaseError::DuplicateTagPattern { .. })),
        "{error:?}"
    );
    files.release_plz_toml.insert(
        "tools".to_owned(),
        workspace_only("tools-", "tools-v{{ version }}"),
    );
    assert_eq!(refusal(&files), None);
    Ok(())
}

const PLZ_TOOLS: &str = "[workspace]\ngit_tag_name = \"{{ package }}-v{{ version }}\"\ngit_release_draft = true\nrelease_always = false\nsemver_check = false\npublish_no_verify = true\npr_branch_prefix = \"tools-\"\n\n[[package]]\nname = \"tools\"\n";

#[test]
fn every_release_root_needs_a_tracked_cargo_manifest() -> TestResult {
    let mut files = two_roots(PLZ_TOOLS)?;
    files.cargo_manifests = Some(names(&["Cargo.toml"]));
    let error = refusal(&files);
    assert!(
        matches!(&error, Some(ReleaseError::RootWithoutCargoManifest { root }) if root == "tools"),
        "{error:?}"
    );
    files.cargo_manifests = Some(names(&["Cargo.toml", "tools/Cargo.toml"]));
    assert_eq!(refusal(&files), None);
    Ok(())
}

#[test]
fn every_after_name_must_be_a_workflow_name() {
    let mut files = files();
    files.release_toml = Some("after = [\"Rust CI\", \"Rust Ci\"]\n".to_owned());
    files.workflow_names = Some(names(&["Rust CI", "Lint"]));
    let error = refusal(&files);
    assert!(
        matches!(&error, Some(ReleaseError::UnknownWorkflow { name }) if name == "Rust Ci"),
        "{error:?}"
    );
    files.release_toml = Some("after = [\"Rust CI\", \"Lint\"]\n".to_owned());
    assert_eq!(refusal(&files), None);
}

#[test]
fn a_binary_without_a_buildable_bin_target_in_its_root_is_refused() {
    let mut files = files();
    files.release_toml = Some("[[binaries]]\npackage = \"app\"\nbin = \"rg\"\n".to_owned());
    let target =
        |manifest: &str, bin: &str| (manifest.to_owned(), "app".to_owned(), bin.to_owned());
    files.bin_targets = Some(BTreeSet::from([target("Cargo.toml", "app")]));
    let error = refusal(&files);
    assert!(
        matches!(&error, Some(ReleaseError::BinaryTargetMissing { package, bin }) if package == "app" && bin == "rg"),
        "{error:?}"
    );
    files.bin_targets = Some(BTreeSet::from([target("other/Cargo.toml", "rg")]));
    assert!(matches!(
        refusal(&files),
        Some(ReleaseError::BinaryTargetMissing { .. })
    ));
    files.bin_targets = Some(BTreeSet::from([target("Cargo.toml", "rg")]));
    assert_eq!(refusal(&files), None);
}

#[test]
fn a_package_name_inherited_from_the_workspace_in_two_roots_is_refused() -> TestResult {
    let mut files = two_roots(PLZ_TOOLS)?;
    with_metadata(
        &mut files,
        &[
            ("Cargo.toml", &[("app", "Cargo.toml")]),
            (
                "tools/Cargo.toml",
                &[
                    ("app", "tools/app/Cargo.toml"),
                    ("tools", "tools/Cargo.toml"),
                ],
            ),
        ],
    );
    let error = refusal(&files);
    assert!(
        matches!(&error, Some(ReleaseError::DuplicatePackageName { package, .. }) if package == "app"),
        "{error:?}"
    );
    with_metadata(
        &mut files,
        &[
            ("Cargo.toml", &[("app", "Cargo.toml")]),
            ("tools/Cargo.toml", &[("tools", "tools/Cargo.toml")]),
        ],
    );
    assert_eq!(refusal(&files), None);
    Ok(())
}

#[test]
fn two_roots_that_share_an_ancestor_workspace_release_its_packages_twice() -> TestResult {
    let mut files = two_roots(PLZ_TOOLS)?;
    with_metadata(
        &mut files,
        &[(
            "Cargo.toml",
            &[("app", "Cargo.toml"), ("tools", "tools/Cargo.toml")],
        )],
    );
    let error = refusal(&files);
    assert!(
        matches!(&error, Some(ReleaseError::DuplicatePackageName { .. })),
        "{error:?}"
    );
    Ok(())
}

#[test]
fn a_package_entry_that_is_not_a_member_of_its_root_is_refused() {
    let mut files = files();
    with_metadata(&mut files, &[("Cargo.toml", &[("other", "Cargo.toml")])]);
    let error = refusal(&files);
    assert!(
        matches!(&error, Some(ReleaseError::PackageNotInRoot { package, .. }) if package == "app"),
        "{error:?}"
    );
    with_metadata(
        &mut files,
        &[(
            "Cargo.toml",
            &[("app", "Cargo.toml"), ("other", "other/Cargo.toml")],
        )],
    );
    assert_eq!(refusal(&files), None);
}

#[test]
fn only_cargo_configs_that_reach_a_release_root_are_checked_for_credential_providers() {
    let provider = "[registry]\ncredential-provider = [\"./steal\"]\n".to_owned();
    let mut files = files();
    files.cargo_configs.insert(
        "examples/demo/.cargo/config.toml".to_owned(),
        provider.clone(),
    );
    assert_eq!(refusal(&files), None);
    files
        .cargo_configs
        .insert(".cargo/config.toml".to_owned(), provider);
    assert!(matches!(
        refusal(&files),
        Some(ReleaseError::CargoCredentialProvider { .. })
    ));
}

#[test]
fn cargo_configs_that_make_cargo_run_a_program_are_refused_whatever_the_root_publishes() {
    for config in [
        "[build]\nrustc-wrapper = \"/tmp/evil\"\n",
        "[build]\nrustc-workspace-wrapper = \"/tmp/evil\"\n",
        "[build]\nrustc = \"/tmp/evil\"\n",
        "[env]\nLD_PRELOAD = \"/tmp/evil.so\"\n",
        "env.LD_PRELOAD = \"/tmp/evil.so\"\n",
    ] {
        let mut files = files();
        files.release_plz_toml.insert(
            ".".to_owned(),
            PLZ.replace(
                "semver_check = false",
                "semver_check = false\npublish = false",
            ),
        );
        files
            .cargo_configs
            .insert(".cargo/config.toml".to_owned(), config.to_owned());
        assert!(
            matches!(refusal(&files), Some(ReleaseError::CargoRunsProgram { .. })),
            "{config}"
        );
    }
}

#[test]
fn cargo_proxy_and_ca_overrides_are_refused_when_publishing_to_crates_io() {
    for config in [
        "[http]\nproxy = \"http://evil.example:8080\"\n",
        "[http]\ncainfo = \"evil.pem\"\n",
    ] {
        let mut files = files();
        files
            .cargo_configs
            .insert(".cargo/config.toml".to_owned(), config.to_owned());
        assert!(
            matches!(
                refusal(&files),
                Some(ReleaseError::CargoRedirectsCratesIo { .. })
            ),
            "{config}"
        );
    }
}

#[test]
fn a_non_crates_io_default_registry_is_refused_when_publishing_to_crates_io() {
    for path in [".cargo/config.toml", ".cargo/config"] {
        for config in [
            "[registry]\ndefault = \"company\"\n[registries.company]\nindex = \"sparse+https://example.test/\"\n",
            "[registry]\ndefault = 7\n",
        ] {
            let mut files = files();
            files
                .cargo_configs
                .insert(path.to_owned(), config.to_owned());
            assert!(
                matches!(
                    refusal(&files),
                    Some(ReleaseError::CargoRedirectsCratesIo { .. })
                ),
                "{path}: {config}"
            );
        }
    }
}

#[test]
fn net_offline_is_refused_only_for_a_root_that_publishes_to_crates_io() {
    let git_only = edit(PLZ, "publish_no_verify = true", "git_only = true");
    let mut publishing = files();
    publishing.cargo_configs.insert(
        ".cargo/config.toml".to_owned(),
        "[net]\noffline = true\n".to_owned(),
    );
    assert!(matches!(
        refusal(&publishing),
        Some(ReleaseError::CargoOffline { .. })
    ));
    publishing.cargo_configs.insert(
        ".cargo/config.toml".to_owned(),
        "[net]\noffline = false\nretry = 3\n".to_owned(),
    );
    assert_eq!(refusal(&publishing), None);
    let mut local = files();
    local
        .release_plz_toml
        .insert(".".to_owned(), git_only.unwrap_or_default());
    local.cargo_configs.insert(
        ".cargo/config.toml".to_owned(),
        "[net]\noffline = true\n".to_owned(),
    );
    assert_eq!(refusal(&local), None);
}

#[test]
fn a_crates_io_default_registry_is_accepted() {
    let mut files = files();
    files.cargo_configs.insert(
        ".cargo/config.toml".to_owned(),
        "[registry]\ndefault = \"crates-io\"\n".to_owned(),
    );
    assert!(refusal(&files).is_none());
}

#[test]
fn a_cargo_config_include_or_unknown_key_is_refused_for_every_root() {
    for (config, include) in [
        ("include = [\"../cfg/extra.toml\"]\n", true),
        ("Include = \"extra.toml\"\n", true),
        ("[[include]]\npath = \"extra.toml\"\n", true),
        ("[alias]\nb = \"build\"\n", false),
        ("[target.x.runner]\nk = 1\n", false),
    ] {
        let mut files = files();
        files
            .cargo_configs
            .insert(".cargo/config.toml".to_owned(), config.to_owned());
        let got = refusal(&files);
        if include {
            assert!(
                matches!(got, Some(ReleaseError::CargoConfigInclude { .. })),
                "{config}: {got:?}"
            );
        } else {
            assert!(
                matches!(got, Some(ReleaseError::CargoConfigKeyNotAllowed { .. })),
                "{config}: {got:?}"
            );
        }
    }
}

#[test]
fn the_refusal_of_an_unknown_cargo_key_names_the_file_and_the_key_path() {
    let mut files = files();
    files.cargo_configs.insert(
        ".cargo/config.toml".to_owned(),
        "[profile.release.package.dep]\nopt-level = 3\nrustflags = [\"x\"]\n".to_owned(),
    );
    let message = refusal(&files).map(|error| error.to_string());
    assert!(
        message
            .as_deref()
            .is_some_and(|text| text.contains(".cargo/config.toml")
                && text.contains("profile.release.package.dep.rustflags")),
        "{message:?}"
    );
}

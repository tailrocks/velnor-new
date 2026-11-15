// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_release::{ReleaseError, ReleaseFiles, ReleasePlan};

use crate::fixture::{PLZ, TestResult, edit, files, refusal};

fn with_mise(text: &str) -> ReleaseFiles {
    let mut files = files();
    files.mise_tomls = vec![text.to_owned()];
    files
}

fn names(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

fn two_roots(tools_plz: &str) -> Result<ReleaseFiles, String> {
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
fn release_plz_must_come_from_an_approved_source_without_extra_options() {
    for text in [
        "[tools.release-plz]\nversion = \"0.4.3\"\npath = \"./fake\"\n",
        "[tools.release-plz]\nversion = \"0.4.3\"\nref = \"main\"\n",
        "[tools.release-plz]\nversion = \"0.4.3\"\nprefix = \"x\"\n",
        "[tools.\"github:evil/release-plz\"]\nversion = \"0.4.3\"\n",
        "[tools.\"cargo:https://example.com/release-plz\"]\nversion = \"0.4.3\"\n",
    ] {
        let error = refusal(&with_mise(text));
        assert!(
            matches!(error, Some(ReleaseError::ReleasePlzSource { .. })),
            "{text}: {error:?}"
        );
    }
}

#[test]
fn the_plan_keeps_the_mise_key_that_matched_the_lock() -> TestResult {
    let mut files = with_mise("[tools]\n\"aqua:release-plz/release-plz\" = \"0.4.3\"\n");
    files.mise_lock =
        "[[tools.\"aqua:release-plz/release-plz\"]]\nversion = \"0.4.3\"\n".to_owned();
    assert_eq!(
        ReleasePlan::read(&files)?.release_plz_tool,
        "aqua:release-plz/release-plz"
    );
    assert_eq!(
        ReleasePlan::read(&crate::fixture::files())?.release_plz_tool,
        "release-plz"
    );
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
        matches!(error, Some(ReleaseError::TagsMayCollide { .. })),
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
fn a_plugin_for_release_plz_or_an_unapproved_locked_backend_is_refused() {
    let plugin = "[tools]\nrelease-plz = \"0.4.3\"\n[plugins]\n\"asdf:release-plz\" = \"https://example.com/evil\"\n";
    let error = refusal(&with_mise(plugin));
    assert!(
        matches!(error, Some(ReleaseError::ReleasePlzSource { .. })),
        "{error:?}"
    );
    let mut files = files();
    files.mise_lock =
        "[[tools.release-plz]]\nversion = \"0.4.3\"\nbackend = \"asdf:release-plz\"\n".to_owned();
    assert!(matches!(
        refusal(&files),
        Some(ReleaseError::ReleasePlzSource { .. })
    ));
    files.mise_lock =
        "[[tools.release-plz]]\nversion = \"0.4.3\"\nbackend = \"aqua:release-plz/release-plz\"\n"
            .to_owned();
    assert_eq!(refusal(&files), None);
    let other = "[tools]\nrelease-plz = \"0.4.3\"\n[plugins]\nbun = \"https://example.com/bun\"\n";
    assert_eq!(refusal(&with_mise(other)), None);
}

#[test]
fn a_package_name_inherited_from_the_workspace_in_two_roots_is_refused() -> TestResult {
    let members = |app: &[&str], tools: &[&str]| {
        Some(std::collections::BTreeMap::from([
            (".".to_owned(), names(app)),
            ("tools".to_owned(), names(tools)),
        ]))
    };
    let mut files = two_roots(PLZ_TOOLS)?;
    files.root_members = members(&["app"], &["app", "tools"]);
    let error = refusal(&files);
    assert!(
        matches!(&error, Some(ReleaseError::DuplicatePackageName { package, .. }) if package == "app"),
        "{error:?}"
    );
    files.root_members = members(&["app"], &["tools"]);
    assert_eq!(refusal(&files), None);
    Ok(())
}

#[test]
fn a_package_entry_that_is_not_a_member_of_its_root_is_refused() {
    let mut files = files();
    files.root_members = Some(std::collections::BTreeMap::from([(
        ".".to_owned(),
        names(&["other"]),
    )]));
    let error = refusal(&files);
    assert!(
        matches!(&error, Some(ReleaseError::PackageNotInRoot { package, .. }) if package == "app"),
        "{error:?}"
    );
    files.root_members = Some(std::collections::BTreeMap::from([(
        ".".to_owned(),
        names(&["app", "other"]),
    )]));
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
fn a_locked_download_must_come_from_the_release_plz_repository() {
    let lock = |url: &str| {
        format!(
            "[[tools.release-plz]]\nversion = \"0.4.3\"\nbackend = \"aqua:release-plz/release-plz\"\n[tools.release-plz.platforms.linux-x64]\nchecksum = \"sha256:00\"\nurl = \"{url}\"\n"
        )
    };
    let mut files = files();
    files.mise_lock = lock("https://example.com/release-plz.tar.gz");
    assert!(matches!(
        refusal(&files),
        Some(ReleaseError::ReleasePlzSource { .. })
    ));
    files.mise_lock = lock(
        "https://github.com/release-plz/release-plz/releases/download/release-plz-v0.4.3/release-plz.tar.gz",
    );
    assert_eq!(refusal(&files), None);
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
                Some(ReleaseError::CargoCredentialProvider { .. })
            ),
            "{config}"
        );
    }
}

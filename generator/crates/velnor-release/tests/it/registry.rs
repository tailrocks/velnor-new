// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::{ReleaseError, ReleaseFiles};

use crate::fixture::{PLZ, TestResult, edit, files, metadata, refusal};
use crate::sources::two_roots;

const OTHER_DEFAULT: &str = "[registry]\ndefault = \"company\"\n[registries.company]\nindex = \"sparse+https://example.test/\"\n";

fn manifest(publish: &str) -> String {
    format!("[package]\nname = \"app\"\nversion = \"0.1.0\"\n{publish}")
}

fn with_manifest(path: &str, text: String) -> ReleaseFiles {
    let mut files = files();
    files.rust_metadata = Some(metadata(&[("Cargo.toml", &[("app", path)])]));
    files.cargo_manifest_texts.insert(path.to_owned(), text);
    files
}

fn plz_with(extra_workspace: &str, packages: &str) -> String {
    let text = edit(
        PLZ,
        "[workspace]\n",
        &format!("[workspace]\n{extra_workspace}"),
    )
    .unwrap_or_default();
    text.replace("[[package]]\nname = \"app\"\n", packages)
}

#[test]
fn a_package_publish_list_naming_another_registry_is_refused() {
    for publish in [
        "publish = [\"company\"]\n",
        "publish = [\"company\", \"crates-io\"]\n",
        "publish = [7]\n",
    ] {
        let files = with_manifest("Cargo.toml", manifest(publish));
        assert!(
            matches!(
                refusal(&files),
                Some(ReleaseError::PublishNamesOtherRegistry { .. })
            ),
            "{publish}"
        );
    }
}

#[test]
fn a_publish_list_inherited_from_the_workspace_is_refused() {
    let mut files = with_manifest("member/Cargo.toml", manifest("publish.workspace = true\n"));
    files.cargo_manifest_texts.insert(
        "Cargo.toml".to_owned(),
        "[workspace.package]\npublish = [\"company\"]\n".to_owned(),
    );
    assert!(matches!(
        refusal(&files),
        Some(ReleaseError::PublishNamesOtherRegistry { manifest, .. }) if manifest == "member/Cargo.toml"
    ));
}

fn ancestor_files(workspace_publish: &str) -> ReleaseFiles {
    let mut files = files();
    files.release_toml = Some("roots = [\"crates/app\"]\n".to_owned());
    let text = files.release_plz_toml.remove(".").unwrap_or_default();
    files.release_plz_toml.insert("crates/app".to_owned(), text);
    files.rust_metadata = Some(metadata(&[(
        "Cargo.toml",
        &[("app", "crates/app/Cargo.toml")],
    )]));
    files.cargo_manifest_texts.insert(
        "Cargo.toml".to_owned(),
        format!(
            "[workspace]\nmembers = [\"crates/app\"]\n[workspace.package]\n{workspace_publish}"
        ),
    );
    files.cargo_manifest_texts.insert(
        "crates/app/Cargo.toml".to_owned(),
        manifest("publish.workspace = true\n"),
    );
    files
}

#[test]
fn a_publish_list_inherited_from_a_workspace_above_the_release_root_is_refused() {
    let error = refusal(&ancestor_files("publish = [\"company\"]\n"));
    assert!(
        matches!(&error, Some(ReleaseError::PublishNamesOtherRegistry { manifest, root, .. })
            if manifest == "crates/app/Cargo.toml" && root == "crates/app"),
        "{error:?}"
    );
    assert_eq!(refusal(&ancestor_files("publish = false\n")), None);
}

#[test]
fn crates_io_only_false_and_absent_publish_fields_are_accepted() {
    for publish in ["", "publish = false\n", "publish = [\"crates-io\"]\n"] {
        let files = with_manifest("Cargo.toml", manifest(publish));
        assert!(refusal(&files).is_none(), "{publish}");
    }
}

#[test]
fn a_manifest_outside_the_cargo_workspace_is_not_a_package_of_the_root() {
    let mut files = with_manifest("Cargo.toml", manifest(""));
    let other = manifest("publish = [\"company\"]\n").replace("\"app\"", "\"demo\"");
    for path in ["examples/demo/Cargo.toml", "nested/Cargo.toml"] {
        files
            .cargo_manifest_texts
            .insert(path.to_owned(), other.clone());
    }
    assert!(refusal(&files).is_none());
}

#[test]
fn a_member_of_a_nested_workspace_is_not_a_package_of_the_outer_root() {
    let mut files = with_manifest("Cargo.toml", manifest(""));
    files.rust_metadata = Some(metadata(&[
        ("Cargo.toml", &[("app", "Cargo.toml")]),
        ("nested/Cargo.toml", &[("demo", "nested/demo/Cargo.toml")]),
    ]));
    files.cargo_manifest_texts.insert(
        "nested/demo/Cargo.toml".to_owned(),
        manifest("publish = [\"company\"]\n").replace("\"app\"", "\"demo\""),
    );
    assert!(refusal(&files).is_none());
}

#[test]
fn a_package_that_release_plz_does_not_publish_is_not_checked() {
    let company = manifest("publish = [\"company\"]\n");
    for opt_out in [
        "[[package]]\nname = \"app\"\n\n[[package]]\nname = \"tool\"\npublish = false\n",
        "[[package]]\nname = \"app\"\n\n[[package]]\nname = \"tool\"\ngit_only = true\n",
        "[[package]]\nname = \"app\"\n\n[[package]]\nname = \"tool\"\nrelease = false\n",
    ] {
        let mut files = with_manifest("Cargo.toml", manifest(""));
        files.rust_metadata = Some(metadata(&[(
            "Cargo.toml",
            &[("app", "Cargo.toml"), ("tool", "tool/Cargo.toml")],
        )]));
        files.cargo_manifest_texts.insert(
            "tool/Cargo.toml".to_owned(),
            company.replace("\"app\"", "\"tool\""),
        );
        files
            .release_plz_toml
            .insert(".".to_owned(), plz_with("", opt_out));
        assert_eq!(refusal(&files), None, "{opt_out}");
        files.release_plz_toml.insert(
            ".".to_owned(),
            plz_with(
                "",
                "[[package]]\nname = \"app\"\n\n[[package]]\nname = \"tool\"\n",
            ),
        );
        assert!(
            matches!(
                refusal(&files),
                Some(ReleaseError::PublishNamesOtherRegistry { .. })
            ),
            "{opt_out}"
        );
    }
}

#[test]
fn a_manifest_the_metadata_lists_but_the_checkout_lacks_is_refused() {
    let mut files = with_manifest("Cargo.toml", manifest(""));
    files.cargo_manifest_texts.clear();
    assert!(matches!(
        refusal(&files),
        Some(ReleaseError::ManifestUnreadable { .. })
    ));
}

#[test]
fn a_publish_list_of_a_package_the_root_does_not_release_is_ignored() {
    let mut files = with_manifest("Cargo.toml", manifest(""));
    files.rust_metadata = Some(metadata(&[(
        "Cargo.toml",
        &[("app", "Cargo.toml"), ("other", "other/Cargo.toml")],
    )]));
    files.cargo_manifest_texts.insert(
        "other/Cargo.toml".to_owned(),
        manifest("publish = [\"company\"]\n").replace("\"app\"", "\"other\""),
    );
    let plz = plz_with(
        "release = false\n",
        "[[package]]\nname = \"app\"\nrelease = true\n",
    );
    files.release_plz_toml.insert(".".to_owned(), plz);
    assert!(refusal(&files).is_none());
    files.cargo_manifest_texts.insert(
        "Cargo.toml".to_owned(),
        manifest("publish = [\"company\"]\n"),
    );
    assert!(matches!(
        refusal(&files),
        Some(ReleaseError::PublishNamesOtherRegistry { .. })
    ));
}

#[test]
fn a_root_that_does_not_publish_accepts_another_registry_everywhere() {
    let mut files = files();
    files.release_plz_toml.insert(
        ".".to_owned(),
        edit(PLZ, "[workspace]\n", "[workspace]\npublish = false\n").unwrap_or_default(),
    );
    files.rust_metadata = Some(metadata(&[("Cargo.toml", &[("app", "Cargo.toml")])]));
    files.cargo_manifest_texts.insert(
        "Cargo.toml".to_owned(),
        manifest("publish = [\"company\"]\n"),
    );
    files
        .cargo_configs
        .insert(".cargo/config.toml".to_owned(), OTHER_DEFAULT.to_owned());
    assert!(refusal(&files).is_none());
}

#[test]
fn a_cargo_config_is_judged_by_the_roots_it_reaches_not_by_other_roots() -> TestResult {
    let mut files = two_roots(
        &edit(
            PLZ,
            "[workspace]\n",
            "[workspace]\npr_branch_prefix = \"tools-\"\npublish = false\n",
        )?
        .replace("name = \"app\"", "name = \"tools\""),
    )?;
    files.cargo_configs.insert(
        "tools/.cargo/config.toml".to_owned(),
        OTHER_DEFAULT.to_owned(),
    );
    assert!(refusal(&files).is_none());
    files.cargo_configs.clear();
    files
        .cargo_configs
        .insert(".cargo/config.toml".to_owned(), OTHER_DEFAULT.to_owned());
    assert!(matches!(
        refusal(&files),
        Some(ReleaseError::CargoRedirectsCratesIo { .. })
    ));
    Ok(())
}

#[test]
fn a_root_no_metadata_workspace_owns_is_refused_not_skipped() {
    let mut files = files();
    files.rust_metadata = Some(metadata(&[(
        "other/Cargo.toml",
        &[("app", "other/Cargo.toml")],
    )]));
    assert!(matches!(
        refusal(&files),
        Some(ReleaseError::RootWithoutWorkspace { .. })
    ));
}

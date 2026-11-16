// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::{ReleaseError, ReleaseFiles};

use crate::fixture::{PLZ, TestResult, edit, files, refusal};
use crate::sources::two_roots;

const OTHER_DEFAULT: &str = "[registry]\ndefault = \"company\"\n[registries.company]\nindex = \"sparse+https://example.test/\"\n";

fn manifest(publish: &str) -> String {
    format!("[package]\nname = \"app\"\nversion = \"0.1.0\"\n{publish}")
}

fn with_manifest(path: &str, text: String) -> ReleaseFiles {
    let mut files = files();
    files.cargo_manifest_texts.insert(path.to_owned(), text);
    files
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
    let mut files = with_manifest(
        "Cargo.toml",
        "[workspace.package]\npublish = [\"company\"]\n".to_owned(),
    );
    files.cargo_manifest_texts.insert(
        "member/Cargo.toml".to_owned(),
        manifest("publish.workspace = true\n"),
    );
    assert!(matches!(
        refusal(&files),
        Some(ReleaseError::PublishNamesOtherRegistry { manifest, .. }) if manifest == "member/Cargo.toml"
    ));
}

#[test]
fn crates_io_only_false_and_absent_publish_fields_are_accepted() {
    for publish in ["", "publish = false\n", "publish = [\"crates-io\"]\n"] {
        let files = with_manifest("Cargo.toml", manifest(publish));
        assert!(refusal(&files).is_none(), "{publish}");
    }
}

#[test]
fn a_publish_list_of_a_package_the_root_does_not_release_is_ignored() {
    let plz = edit(PLZ, "[workspace]\n", "[workspace]\nrelease = false\n")
        .and_then(|text| {
            edit(
                &text,
                "name = \"app\"\n",
                "name = \"app\"\nrelease = true\n",
            )
        })
        .unwrap_or_default();
    let mut files = with_manifest(
        "Cargo.toml",
        manifest("publish = [\"company\"]\n").replace("\"app\"", "\"other\""),
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

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::{ReleaseError, ReleaseFiles};

use crate::fixture::{PLZ, TestResult, edit, files, refusal};

const PROVIDER: &str = "[registry]\ncredential-provider = [\"./steal\"]\n";

fn two_roots() -> Result<ReleaseFiles, String> {
    let mut files = files();
    files.release_toml = Some("roots = [\".\", \"tools\"]\n".to_owned());
    files.release_plz_toml.insert(
        ".".to_owned(),
        edit(
            PLZ,
            "[workspace]\n",
            "[workspace]\npr_branch_prefix = \"app-\"\n",
        )?,
    );
    files.release_plz_toml.insert(
        "tools".to_owned(),
        edit(
            PLZ,
            "[workspace]\n",
            "[workspace]\npr_branch_prefix = \"tools-\"\n",
        )?
        .replace("\"app\"", "\"tools\""),
    );
    Ok(files)
}

fn linked(files: &mut ReleaseFiles, path: &str) {
    files.symlinks.insert(path.to_owned());
}

#[test]
fn a_symlink_that_can_hide_a_cargo_config_from_the_generator_is_refused() -> TestResult {
    for path in [
        ".cargo",
        ".cargo/config",
        ".cargo/config.toml",
        "tools/.cargo",
        "tools/.cargo/config.toml",
        "tools",
    ] {
        let mut files = two_roots()?;
        files
            .cargo_configs
            .insert("cfg/config.toml".to_owned(), PROVIDER.to_owned());
        linked(&mut files, path);
        let got = refusal(&files);
        assert!(
            matches!(&got, Some(ReleaseError::SymlinkOnCargoConfigPath { path: found, .. }) if found == path),
            "{path}: {got:?}"
        );
    }
    Ok(())
}

#[test]
fn the_refusal_names_the_symlink() {
    let mut files = files();
    linked(&mut files, ".cargo");
    let text = refusal(&files).map(|error| error.to_string());
    assert!(
        text.as_deref()
            .is_some_and(|text| text.starts_with(".cargo: ")),
        "{text:?}"
    );
}

#[test]
fn symlinks_off_the_discovery_path_of_every_root_are_left_alone() -> TestResult {
    let mut files = two_roots()?;
    for path in [
        "examples/.cargo",
        "examples/demo/.cargo/config.toml",
        "other",
        "docs/cfg",
        "tools2",
        "cfg",
    ] {
        linked(&mut files, path);
    }
    assert_eq!(refusal(&files), None);
    Ok(())
}

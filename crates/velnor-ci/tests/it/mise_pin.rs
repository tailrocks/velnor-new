// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;

use velnor_ci::mise_pin::{append_output, pinned_version};

use crate::io::Repo;

const TOOL: &str = "mr-boxington";

fn workspace(name: &str, files: &[(&str, &str)]) -> Repo {
    let repo = Repo::empty(name);
    for (file, text) in files {
        fs::write(repo.root().join(file), text).unwrap_or_else(|e| unreachable!("{e}"));
    }
    repo
}

fn pin(name: &str, mise: &str) -> Result<String, String> {
    let repo = workspace(name, &[("mise.toml", mise)]);
    pinned_version(repo.root(), TOOL).map_err(|e| e.to_string())
}

#[test]
fn a_v_prefixed_exact_pin_prints_the_bare_release() {
    assert_eq!(
        pin("mise-pin-v", "[tools]\nmr-boxington = \"v1.31.0\"\n").as_deref(),
        Ok("1.31.0")
    );
}

#[test]
fn a_bare_exact_pin_prints_the_same_release() {
    assert_eq!(
        pin("mise-pin-bare", "[tools]\nmr-boxington = \"1.31.0\"\n").as_deref(),
        Ok("1.31.0")
    );
}

#[test]
fn the_table_form_with_a_version_is_read() {
    let mise = "[tools]\nmr-boxington = { version = \"v1.30.0\", workflow = \"release.yml@refs/heads/main\" }\n";
    assert_eq!(pin("mise-pin-table", mise).as_deref(), Ok("1.30.0"));
}

#[test]
fn a_missing_pin_fails_closed() {
    let mise = "[tools]\nrust = \"1.99.1\"\n";
    assert!(pin("mise-pin-missing", mise).is_err());
}

#[test]
fn a_missing_config_fails_closed() {
    let repo = workspace("mise-pin-no-config", &[]);
    assert!(pinned_version(repo.root(), TOOL).is_err());
}

#[test]
fn ranges_latest_and_loose_versions_fail_closed() {
    for version in [
        "latest",
        "^1.30.0",
        "~1.30.0",
        ">=1.30.0",
        "1.30",
        "1",
        "1.30.0-rc.1",
        "01.30.0",
        "1.30.00",
        "v1.30.x",
    ] {
        let mise = format!("[tools]\nmr-boxington = \"{version}\"\n");
        assert!(
            pin("mise-pin-loose", &mise).is_err(),
            "{version} was accepted"
        );
    }
}

#[test]
fn two_different_pins_fail_closed() {
    let mise = "[tools]\nmr-boxington = [\"1.30.0\", \"1.31.0\"]\n";
    assert!(pin("mise-pin-array", mise).is_err());
}

#[test]
fn a_second_config_that_disagrees_fails_closed() {
    let repo = workspace(
        "mise-pin-two-configs",
        &[
            ("mise.toml", "[tools]\nmr-boxington = \"v1.30.0\"\n"),
            (".mise.toml", "[tools]\nmr-boxington = \"v1.31.0\"\n"),
        ],
    );
    assert!(pinned_version(repo.root(), TOOL).is_err());
}

#[test]
fn a_lock_row_for_the_same_release_agrees() {
    let lock = "[[tools.mr-boxington]]\nversion = \"1.31.0\"\nbackend = \"packslip:github.com/jdx/mr-boxington\"\nspecifiers = [\"v1.31.0\"]\n";
    let repo = workspace(
        "mise-pin-lock-ok",
        &[
            ("mise.toml", "[tools]\nmr-boxington = \"v1.31.0\"\n"),
            ("mise.lock", lock),
        ],
    );
    assert_eq!(pinned_version(repo.root(), TOOL).as_deref(), Ok("1.31.0"));
}

#[test]
fn a_lock_for_another_release_fails_closed() {
    let lock = "[[tools.mr-boxington]]\nversion = \"1.30.0\"\n";
    let repo = workspace(
        "mise-pin-lock-stale",
        &[
            ("mise.toml", "[tools]\nmr-boxington = \"v1.31.0\"\n"),
            ("mise.lock", lock),
        ],
    );
    assert!(pinned_version(repo.root(), TOOL).is_err());
}

#[test]
fn a_lock_without_a_row_for_the_tool_fails_closed() {
    let repo = workspace(
        "mise-pin-lock-missing",
        &[
            ("mise.toml", "[tools]\nmr-boxington = \"v1.31.0\"\n"),
            ("mise.lock", "[[tools.rust]]\nversion = \"1.99.1\"\n"),
        ],
    );
    assert!(pinned_version(repo.root(), TOOL).is_err());
}

#[test]
fn the_output_file_receives_one_version_line() {
    let repo = workspace("mise-pin-output", &[]);
    let file = repo.root().join("github-output");
    append_output(&file, "1.31.0").unwrap_or_else(|e| unreachable!("{e}"));
    let text = fs::read_to_string(Path::new(&file)).unwrap_or_default();
    assert_eq!(text, "version=1.31.0\n");
}

//! `rust-toolchain.toml` must mirror the `mise.toml` rust entry.

use std::path::Path;

use super::{MIRROR_CODE, check_toolchain_mirror};

const MISE: &str = "[tools]\nrust = { version = \"1.99.1\", components = [\"rustfmt\", \"clippy\"], targets = [\"x86_64-unknown-linux-gnu\", \"aarch64-unknown-linux-gnu\"], mr_boxington = true }\n";
const TOOLCHAIN: &str = "[toolchain]\nchannel = \"1.99.1\"\ncomponents = [\"clippy\", \"rustfmt\"]\ntargets = [\"aarch64-unknown-linux-gnu\", \"x86_64-unknown-linux-gnu\"]\n";

fn repo(mise: Option<&str>, toolchain: Option<&str>) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let write = |rel: &str, text: &str| std::fs::write(dir.path().join(rel), text).expect("write");
    if let Some(text) = mise {
        write("mise.toml", text);
    }
    if let Some(text) = toolchain {
        write("rust-toolchain.toml", text);
    }
    dir
}

fn error(root: &Path) -> String {
    check_toolchain_mirror(root)
        .expect_err("the mirror must differ")
        .to_string()
}

#[test]
fn identical_entries_in_any_order_pass() {
    assert!(check_toolchain_mirror(repo(Some(MISE), Some(TOOLCHAIN)).path()).is_ok());
}

#[test]
fn either_file_alone_passes() {
    assert!(check_toolchain_mirror(repo(Some(MISE), None).path()).is_ok());
    assert!(check_toolchain_mirror(repo(None, Some(TOOLCHAIN)).path()).is_ok());
    assert!(
        check_toolchain_mirror(repo(Some("[tools]\nnode = \"24.0.0\"\n"), Some(TOOLCHAIN)).path())
            .is_ok()
    );
}

#[test]
fn components_missing_from_the_mise_entry_fail() {
    let dir = repo(
        Some("[tools]\nrust = { version = \"1.99.1\", mr_boxington = true }\n"),
        Some(TOOLCHAIN),
    );
    let err = error(dir.path());
    assert!(
        err.contains(&format!(
            "{MIRROR_CODE}:components:rust-toolchain.toml=[clippy,rustfmt],mise.toml=[]"
        )),
        "{err}"
    );
}

#[test]
fn targets_profile_and_channel_drift_fail_naming_the_key() {
    let dir = repo(
        Some(MISE),
        Some(&TOOLCHAIN.replace(", \"x86_64-unknown-linux-gnu\"", "")),
    );
    assert!(error(dir.path()).contains(&format!("{MIRROR_CODE}:targets:")));

    let dir = repo(
        Some(MISE),
        Some(&format!("{TOOLCHAIN}profile = \"minimal\"\n")),
    );
    assert!(error(dir.path()).contains(&format!(
        "{MIRROR_CODE}:profile:rust-toolchain.toml=[minimal],mise.toml=[]"
    )));

    let dir = repo(Some(MISE), Some(&TOOLCHAIN.replace("1.99.1", "1.98.1")));
    assert!(error(dir.path()).contains(&format!("{MIRROR_CODE}:channel:")));
}

#[test]
fn a_plain_string_mise_pin_mirrors_a_bare_toolchain_file() {
    let dir = repo(
        Some("[tools]\nrust = \"1.99.1\"\n"),
        Some("[toolchain]\nchannel = \"1.99.1\"\n"),
    );
    assert!(check_toolchain_mirror(dir.path()).is_ok());
}

/// A repository holding exactly `files`.
fn repo_with(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    for (rel, text) in files {
        let path = dir.path().join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("dirs");
        }
        std::fs::write(path, text).expect("write");
    }
    dir
}

#[test]
fn the_documented_comma_string_form_mirrors_an_array() {
    // mise documents `components`/`targets` "as an array or a
    // comma-separated string"; the string form used to compare as one item.
    let mise = "[tools]\nrust = { version = \"1.99.1\", components = \"clippy,rustfmt\", targets = \"aarch64-unknown-linux-gnu, x86_64-unknown-linux-gnu\" }\n";
    assert!(check_toolchain_mirror(repo(Some(mise), Some(TOOLCHAIN)).path()).is_ok());
    let drifted = mise.replace("clippy,rustfmt", "clippy");
    let err = error(repo(Some(&drifted), Some(TOOLCHAIN)).path());
    assert!(
        err.contains(&format!(
            "{MIRROR_CODE}:components:rust-toolchain.toml=[clippy,rustfmt],mise.toml=[clippy]"
        )),
        "{err}"
    );
}

#[test]
fn every_mise_config_location_mise_reads_is_compared() {
    // Drift in a config Mise reads used to pass because only `mise.toml`
    // and `.mise.toml` were compared.
    let drifted = MISE.replace(", \"aarch64-unknown-linux-gnu\"", "");
    for file in [
        ".config/mise.toml",
        "mise/config.toml",
        ".mise/config.toml",
        ".config/mise/conf.d/rust.toml",
        "mise.ci.toml",
    ] {
        let dir = repo_with(&[(file, &drifted), ("rust-toolchain.toml", TOOLCHAIN)]);
        let err = error(dir.path());
        assert!(
            err.contains(&format!("{MIRROR_CODE}:targets:rust-toolchain.toml="))
                && err.contains(&format!("{file}=[")),
            "{file}: {err}"
        );
        let agreeing = repo_with(&[(file, MISE), ("rust-toolchain.toml", TOOLCHAIN)]);
        assert!(check_toolchain_mirror(agreeing.path()).is_ok(), "{file}");
    }
}

#[test]
fn nested_and_legacy_toolchain_files_are_compared_key_by_key() {
    let nested = repo_with(&[
        ("mise.toml", MISE),
        ("rust-toolchain.toml", TOOLCHAIN),
        (
            "crates/sub/rust-toolchain.toml",
            "[toolchain]\nchannel = \"1.99.1\"\ncomponents = [\"clippy\"]\ntargets = [\"aarch64-unknown-linux-gnu\", \"x86_64-unknown-linux-gnu\"]\n",
        ),
    ]);
    let err = error(nested.path());
    assert!(
        err.contains(&format!(
            "{MIRROR_CODE}:components:crates/sub/rust-toolchain.toml=[clippy],mise.toml=[clippy,rustfmt]"
        )),
        "{err}"
    );
    // A legacy plain `rust-toolchain` names only a channel, so it cannot
    // mirror an entry with components.
    let plain = repo_with(&[("mise.toml", MISE), ("rust-toolchain", "1.99.1\n")]);
    assert!(error(plain.path()).contains(&format!("{MIRROR_CODE}:components:rust-toolchain=[]")));
    let plain_ok = repo_with(&[
        ("mise.toml", "[tools]\nrust = \"1.99.1\"\n"),
        ("rust-toolchain", "1.99.1\n"),
    ]);
    assert!(check_toolchain_mirror(plain_ok.path()).is_ok());
    let legacy_toml = repo_with(&[("mise.toml", MISE), ("rust-toolchain", TOOLCHAIN)]);
    assert!(check_toolchain_mirror(legacy_toml.path()).is_ok());
}

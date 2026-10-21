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

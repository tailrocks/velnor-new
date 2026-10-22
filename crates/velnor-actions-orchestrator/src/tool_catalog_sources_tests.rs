//! Project tool source resolution tests (hard decision 5).
//!
//! Declared via `#[path]` from `tool_catalog_resolve.rs` under `cfg(test)`.

use std::collections::BTreeMap;

use velnor_actions_mise::PinnedTool;

use super::resolve_tool_catalog;
use crate::pins::tests::config_with;

#[test]
fn a_malformed_mise_config_fails_resolution_in_every_spelling() {
    // Hard decision 5: a source that does not parse is never "declares
    // nothing". Falling back to another source or to a Velnor pin would
    // hide a config Mise itself refuses.
    for (file, text) in [
        ("mise.toml", "[tools\nrust = \n"),
        (".mise.toml", "[tools\nrust = \n"),
        (
            "mise.toml",
            "[tools\nrust = \nmr_boxington = true # stray option line\n",
        ),
        (".config/mise.toml", "[tools\n"),
        (".config/mise/conf.d/a.toml", "[tools\n"),
        ("mise.local.toml", "x = \n"),
        ("rust-toolchain.toml", "[toolchain\n"),
    ] {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(file);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&path, text).expect("write bad");
        if file != "rust-toolchain.toml" {
            // A valid toolchain file must not rescue the malformed config.
            std::fs::write(
                dir.path().join("rust-toolchain.toml"),
                "[toolchain]\nchannel = \"1.99.1\"\n",
            )
            .expect("write toolchain");
        }
        let cfg = config_with(BTreeMap::new());
        let err = resolve_tool_catalog(dir.path(), &cfg).expect_err("malformed must fail");
        let text = err.to_string();
        assert!(
            text.contains("malformed_project_tool_source")
                || text.contains("invalid_rust_toolchain"),
            "{file}: {text}"
        );
        assert!(text.contains(file), "{file} named: {text}");
    }
}

fn resolve_with(files: &[(&str, &str)]) -> Result<velnor_actions_mise::ToolCatalog, String> {
    let dir = tempfile::tempdir().expect("tempdir");
    for (rel, text) in files {
        let path = dir.path().join(rel);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, text).expect("write");
    }
    resolve_tool_catalog(dir.path(), &config_with(BTreeMap::new())).map_err(|err| err.to_string())
}

#[test]
fn declarations_in_every_mise_location_are_project_owned() {
    // A Velnor pin must never silently override a declaration Mise reads.
    for (file, text) in [
        (".config/mise.toml", "[tools]\njactionlint = \"2.2.0\"\n"),
        (
            ".config/mise/config.toml",
            "[tools]\njactionlint = \"2.2.0\"\n",
        ),
        (
            ".config/mise/conf.d/10-lint.toml",
            "[tools]\njactionlint = \"2.2.0\"\n",
        ),
        ("mise/config.toml", "[tools]\njactionlint = \"2.2.0\"\n"),
        (".mise/config.toml", "[tools]\njactionlint = \"2.2.0\"\n"),
        ("mise.local.toml", "[tools]\njactionlint = \"2.2.0\"\n"),
        ("mise.ci.toml", "[tools]\njactionlint = \"2.2.0\"\n"),
        (".mise.ci.toml", "[tools]\njactionlint = \"2.2.0\"\n"),
        (".tool-versions", "jactionlint 2.2.0\n"),
        (".tool-versions", "jactionlint 2.2.0 # pinned\n"),
    ] {
        let catalog = resolve_with(&[(file, text)]).expect("resolve");
        assert!(
            catalog.is_project_declared(PinnedTool::Jactionlint),
            "{file}"
        );
        assert_eq!(catalog.version(PinnedTool::Jactionlint), "2.2.0", "{file}");
        assert!(
            !catalog.is_project_declared(PinnedTool::Shellcheck),
            "{file}"
        );
    }
    let both = resolve_with(&[
        (".config/mise.toml", "[tools]\njactionlint = \"2.2.0\"\n"),
        (".tool-versions", "shellcheck 0.10.0\n"),
    ])
    .expect("resolve");
    assert_eq!(both.version(PinnedTool::Jactionlint), "2.2.0");
    assert_eq!(both.version(PinnedTool::Shellcheck), "0.10.0");
}

#[test]
fn extensionless_toml_rust_toolchain_declares_rust() {
    let catalog = resolve_with(&[("rust-toolchain", "[toolchain]\nchannel = \"1.99.1\"\n")])
        .expect("resolve");
    assert!(catalog.is_project_declared(PinnedTool::Rust));
    assert_eq!(catalog.version(PinnedTool::Rust), "1.99.1");
}

#[test]
fn a_loose_or_templated_version_fails_generation_naming_tool_and_file() {
    for (file, key, value) in [
        ("mise.toml", "aqua:nextest-rs/nextest/cargo-nextest", "0.9"),
        (".config/mise.toml", "shellcheck", "latest"),
        ("mise.local.toml", "rust", "stable"),
        ("mise.toml", "jactionlint", "^2.3.0"),
        ("mise.toml", "opentofu", "01.13.1"),
        ("mise.toml", "shellcheck", "{{ exec(command='id') }}"),
    ] {
        let text = format!("[tools]\n\"{key}\" = \"{value}\"\n");
        let err = resolve_with(&[(file, &text)]).expect_err("loose version must fail");
        assert!(
            err.contains("project_tool_version_not_exact:tool=")
                && err.contains(&format!("file={file}"))
                && err.contains(&format!("key={key}"))
                && err.contains(&format!("value={value}")),
            "{file} {key}={value}: {err}"
        );
        assert!(err.contains(file), "{err}");
    }
    let err =
        resolve_with(&[("mise.toml", "[tools]\nshellcheck = [\"0.11.0\"]\n")]).expect_err("array");
    assert!(
        err.contains("project_tool_version_not_exact:tool=shellcheck"),
        "{err}"
    );
    let err =
        resolve_with(&[(".tool-versions", "shellcheck 0.11.0 0.10.0\n")]).expect_err("fallback");
    assert!(err.contains("tool=shellcheck,file=.tool-versions"), "{err}");
}

#[test]
fn disagreeing_sources_fail_generation_naming_both() {
    let err = resolve_with(&[
        ("mise.toml", "[tools]\njactionlint = \"2.3.0\"\n"),
        (
            ".config/mise/conf.d/x.toml",
            "[tools]\njactionlint = \"2.2.0\"\n",
        ),
    ])
    .expect_err("disagree");
    assert!(
        err.contains("conflicting_project_tool_versions:tool=jactionlint")
            && err.contains("mise.toml jactionlint=2.3.0")
            && err.contains(".config/mise/conf.d/x.toml jactionlint=2.2.0"),
        "{err}"
    );
    // Agreement is fine, whatever the spelling.
    assert!(
        resolve_with(&[
            ("mise.toml", "[tools]\njactionlint = \"2.2.0\"\n"),
            (".tool-versions", "jactionlint 2.2.0\n"),
        ])
        .is_ok()
    );
}

#[test]
fn nested_toolchains_must_agree_with_the_root_declaration() {
    let root = ("mise.toml", "[tools]\nrust = \"1.99.1\"\n");
    // Inert: a nested file declaring what the root declares.
    resolve_with(&[
        root,
        (
            "crates/sub/rust-toolchain.toml",
            "[toolchain]\nchannel = \"1.99.1\"\n",
        ),
    ])
    .expect("agreeing nested toolchain");
    for (nested, text) in [
        (
            "crates/sub/rust-toolchain.toml",
            "[toolchain]\nchannel = \"1.98.0\"\n",
        ),
        ("deep/er/rust-toolchain", "1.98.0\n"),
    ] {
        let err = resolve_with(&[root, (nested, text)]).expect_err("divergent nested");
        assert!(
            err.contains(&format!(
                "nested_rust_toolchain_not_supported:{nested}=1.98.0,root=1.99.1"
            )),
            "{err}"
        );
    }
    // Nothing declared at the root: a nested file is still not honored.
    let err = resolve_with(&[(
        "sub/rust-toolchain.toml",
        "[toolchain]\nchannel = \"1.98.0\"\n",
    )])
    .expect_err("nested without root");
    assert!(err.contains("root=undeclared"), "{err}");
    let err = resolve_with(&[root, ("sub/rust-toolchain.toml", "[toolchain\n")]).expect_err("bad");
    assert!(err.contains("malformed_project_tool_source"), "{err}");
}

#[test]
fn symlinked_sources_fail_resolution() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("real.toml"), "[tools]\nrust = \"1.99.1\"\n").expect("write");
    for link in ["mise.local.toml", ".tool-versions"] {
        let path = dir.path().join(link);
        std::os::unix::fs::symlink("real.toml", &path).expect("symlink");
        let err = resolve_tool_catalog(dir.path(), &config_with(BTreeMap::new()))
            .expect_err("symlink must fail");
        assert!(
            err.to_string().to_lowercase().contains("symlink"),
            "{link}: {err}"
        );
        std::fs::remove_file(path).expect("remove");
    }
}

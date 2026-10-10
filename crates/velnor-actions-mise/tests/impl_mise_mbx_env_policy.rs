//! MBX child environment preserves the catalog-selected Rustup path.

use std::ffi::OsString;
use velnor_actions_mise::command::RUSTUP_TOOLCHAIN_ENV;
use velnor_actions_mise::steps::ToolHomes;
use velnor_actions_mise::{PinnedTool, PinnedToolExec, ToolCatalog};

fn pair(key: &str, value: &str) -> (OsString, OsString) {
    (OsString::from(key), OsString::from(value))
}

fn has(env: &[(OsString, OsString)], key: &str) -> bool {
    env.iter().any(|(name, _)| name == key)
}

#[test]
fn mbx_child_uses_rustup_path_without_mise_cargo_wrappers_or_shims() -> Result<(), String> {
    let request = PinnedToolExec::new(
        vec![PinnedTool::Rust, PinnedTool::MrBoxington],
        std::ffi::OsStr::new("mbx"),
        vec![OsString::from("metadata")],
    )
    .map_err(|err| err.to_string())?;
    let catalog = ToolCatalog::pinned();
    let homes = ToolHomes::new("/Users/alex/owned-rustup", "/Users/alex/owned-cargo")
        .map_err(|err| err.to_string())?;
    let command = request
        .command(&catalog)
        .map_err(|err| err.to_string())?
        .with_env(&homes.env(&catalog))
        .map_err(|err| err.to_string())?;
    let original_path = std::env::join_paths([
        "/Users/alex/.local/share/mise/command-wrappers/bin",
        "/Users/alex/.local/share/mise/shims",
        "/Users/alex/.cargo/bin",
        "/Users/alex/scratch/mise/command-wrappers/bin",
        "/Users/alex/scratch/mise/shims",
        "/opt/other/command-wrappers/bin",
        "/usr/bin",
    ])
    .map_err(|err| err.to_string())?;
    let parent = vec![
        (OsString::from("HOME"), OsString::from("/Users/alex")),
        (
            OsString::from("CARGO"),
            OsString::from("/Users/alex/.rustup/toolchains/1.99.0/bin/cargo"),
        ),
        (
            OsString::from("RUSTC"),
            OsString::from("/Users/alex/.rustup/toolchains/1.99.0/bin/rustc"),
        ),
        (
            OsString::from("RUSTDOC"),
            OsString::from("/Users/alex/.rustup/toolchains/1.99.0/bin/rustdoc"),
        ),
        (
            OsString::from("CARGO_HOME"),
            OsString::from("/Users/alex/scratch/cargo-home"),
        ),
        (
            OsString::from("RUSTUP_HOME"),
            OsString::from("/Users/alex/.rustup"),
        ),
        (
            OsString::from(RUSTUP_TOOLCHAIN_ENV),
            OsString::from("1.97.1"),
        ),
        (
            OsString::from("MISE_DATA_DIR"),
            OsString::from("/Users/alex/.local/share/mise"),
        ),
        (OsString::from("PATH"), original_path),
        (OsString::from("GITHUB_TOKEN"), OsString::from("sentinel")),
        (
            OsString::from("CARGO_REGISTRY_TOKEN"),
            OsString::from("sentinel"),
        ),
    ];

    let child = command.spawn_env(&parent);
    let child_path = child
        .iter()
        .rev()
        .find(|(key, _)| key == "PATH")
        .map(|(_, value)| value)
        .ok_or_else(|| "MBX child PATH was removed".to_owned())?;
    let paths = std::env::split_paths(child_path).collect::<Vec<_>>();
    assert_eq!(
        paths,
        vec![
            std::path::PathBuf::from("/Users/alex/.cargo/bin"),
            std::path::PathBuf::from("/Users/alex/scratch/mise/command-wrappers/bin"),
            std::path::PathBuf::from("/Users/alex/scratch/mise/shims"),
            std::path::PathBuf::from("/opt/other/command-wrappers/bin"),
            std::path::PathBuf::from("/usr/bin"),
        ],
        "only canonical Mise Cargo wrappers and shims are removed"
    );
    for key in ["CARGO", "RUSTC", "RUSTDOC"] {
        assert!(
            !has(&child, key),
            "ambient {key} executable must not override the Mise-selected tool"
        );
    }
    let rustup_selectors: Vec<OsString> = child
        .iter()
        .filter(|(key, _)| key == RUSTUP_TOOLCHAIN_ENV)
        .map(|(_, value)| value.clone())
        .collect();
    assert_eq!(
        rustup_selectors,
        [OsString::from(catalog.rustup_toolchain())],
        "the inherited selector is stripped, while the explicit typed Rust pin survives"
    );
    assert!(
        child
            .iter()
            .any(|(key, value)| key == "MISE_RUSTUP_HOME" && value == "/Users/alex/owned-rustup"),
        "the catalog-owned Rustup home survives"
    );
    assert!(
        child
            .iter()
            .any(|(key, value)| key == "MISE_CARGO_HOME" && value == "/Users/alex/owned-cargo"),
        "the catalog-owned Cargo home survives"
    );
    assert!(
        has(&child, "CARGO_HOME"),
        "isolated Cargo cache home is preserved"
    );
    assert!(has(&child, "RUSTUP_HOME"), "Rustup home is preserved");
    assert!(!has(&child, "GITHUB_TOKEN"));
    assert!(!has(&child, "CARGO_REGISTRY_TOKEN"));
    assert!(command.disables_auto_install());
    Ok(())
}


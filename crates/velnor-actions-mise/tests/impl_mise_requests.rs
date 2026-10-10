//! Typed mise request cases.
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use velnor_actions_mise::{
    MetadataDiscovery, MetadataQualification, MiseError, PinnedTool, PinnedToolExec, ToolCatalog,
};

fn pinned() -> ToolCatalog {
    ToolCatalog::pinned()
}

fn strings(items: &[&str]) -> Vec<OsString> {
    items.iter().map(OsString::from).collect()
}

#[test]
fn discovery_argv_is_byte_exact() -> Result<(), String> {
    let manifest = PathBuf::from("/tmp/x y/Cargo.toml");
    let request = MetadataDiscovery::new(manifest).map_err(|err| err.to_string())?;
    assert_eq!(
        request.argv(&pinned()),
        strings(&[
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "rust@1.99.0",
            "mr-boxington@1.23.0",
            "--",
            "mbx",
            "+1.99.0",
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
            "/tmp/x y/Cargo.toml",
        ])
    );
    let argv = request.argv(&pinned());
    assert!(argv.iter().any(|arg| arg == "mr-boxington@1.23.0"));
    assert!(argv.iter().any(|arg| arg == "mbx"));
    assert!(!argv.iter().any(|arg| arg == "cargo"));
    assert!(
        !argv
            .iter()
            .any(|arg| arg == "--locked" || arg == "--offline"),
        "discovery must not wait for full resolution"
    );
    assert!(
        argv.iter().any(|arg| arg == "/tmp/x y/Cargo.toml"),
        "spaced manifest stays one argument"
    );
    Ok(())
}

#[test]
fn discovery_empty_manifest_rejected() {
    assert!(matches!(
        MetadataDiscovery::new(PathBuf::new()),
        Err(MiseError::InvalidManifestPath { .. })
    ));
}

#[test]
fn discovery_command_matches_argv() -> Result<(), String> {
    let request =
        MetadataDiscovery::new(PathBuf::from("Cargo.toml")).map_err(|err| err.to_string())?;
    let command = request.command(&pinned()).map_err(|err| err.to_string())?;
    assert_eq!(command.argv(), request.argv(&pinned()));
    assert_eq!(command.program(), "mise");
    assert!(format!("{command:?}").contains("Mbx"));
    Ok(())
}

#[test]
fn qualification_argv_carries_locked_offline() -> Result<(), String> {
    let request = MetadataQualification::new(PathBuf::from("/repo/Cargo.toml"))
        .map_err(|err| err.to_string())?;
    assert_eq!(
        request.argv(&pinned()),
        strings(&[
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "rust@1.99.0",
            "mr-boxington@1.23.0",
            "--",
            "mbx",
            "+1.99.0",
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--offline",
            "--manifest-path",
            "/repo/Cargo.toml",
        ])
    );
    let argv = request.argv(&pinned());
    assert!(argv.iter().any(|arg| arg == "mr-boxington@1.23.0"));
    assert!(argv.iter().any(|arg| arg == "mbx"));
    assert!(!argv.iter().any(|arg| arg == "cargo"));
    assert!(
        !argv.iter().any(|arg| arg == "--no-deps"),
        "qualification resolves with full dependencies"
    );
    Ok(())
}

#[test]
fn qualification_empty_manifest_rejected() {
    assert!(matches!(
        MetadataQualification::new(PathBuf::new()),
        Err(MiseError::InvalidManifestPath { .. })
    ));
}

#[test]
fn pinned_exec_selects_exact_tools() -> Result<(), String> {
    let request = PinnedToolExec::new(
        vec![PinnedTool::Rust, PinnedTool::MrBoxington],
        OsStr::new("mbx"),
        strings(&["clippy", "--package", "demo", "--locked"]),
    )
    .map_err(|err| err.to_string())?;
    assert_eq!(
        request.argv(&pinned()),
        strings(&[
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "rust@1.99.0",
            "mr-boxington@1.23.0",
            "--",
            "mbx",
            "clippy",
            "--package",
            "demo",
            "--locked",
        ])
    );
    let argv = request.argv(&pinned());
    assert!(
        !argv
            .iter()
            .any(|arg| arg.to_string_lossy().starts_with("mbx@")),
        "tool selector uses the registry name, not the binary"
    );
    Ok(())
}

#[test]
fn pinned_exec_accepts_all_catalog_tools() -> Result<(), String> {
    let request = PinnedToolExec::new(
        PinnedTool::ALL.to_vec(),
        OsStr::new("gh"),
        strings(&["--version"]),
    )
    .map_err(|err| err.to_string())?;
    let argv = request.argv(&pinned());
    for spec in [
        "rust@1.99.0",
        "mr-boxington@1.23.0",
        "gh@2.102.0",
        "actionlint@1.7.12",
        "shellcheck@0.11.0",
        "zizmor@1.30.1",
        "aqua:nextest-rs/nextest/cargo-nextest@0.9.148",
        "opentofu@1.13.1",
    ] {
        assert!(argv.iter().any(|arg| arg == spec), "missing spec: {spec}");
    }
    Ok(())
}

#[test]
fn pinned_exec_runs_nextest_without_preinstalled_tools() -> Result<(), String> {
    let request = PinnedToolExec::new(
        vec![
            PinnedTool::Rust,
            PinnedTool::MrBoxington,
            PinnedTool::Nextest,
        ],
        OsStr::new("mbx"),
        strings(&[
            "+1.99.0",
            "nextest",
            "run",
            "--locked",
            "--offline",
            "--manifest-path",
            "Cargo.toml",
        ]),
    )
    .map_err(|err| err.to_string())?;
    assert_eq!(
        request.argv(&pinned()),
        strings(&[
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "rust@1.99.0",
            "mr-boxington@1.23.0",
            "aqua:nextest-rs/nextest/cargo-nextest@0.9.148",
            "--",
            "mbx",
            "+1.99.0",
            "nextest",
            "run",
            "--locked",
            "--offline",
            "--manifest-path",
            "Cargo.toml",
        ])
    );
    Ok(())
}

#[test]
fn pinned_exec_rejects_empty_toolchain_and_program() {
    assert!(matches!(
        PinnedToolExec::new(Vec::new(), OsStr::new("cargo"), Vec::new()),
        Err(MiseError::EmptyToolchain)
    ));
    assert!(matches!(
        PinnedToolExec::new(vec![PinnedTool::Rust], OsStr::new(""), Vec::new()),
        Err(MiseError::EmptyCommand { .. })
    ));
}

//! Typed mise request cases.
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use velnor_actions_mise::{
    MetadataDiscovery, MetadataQualification, MiseError, PinnedTool, PinnedToolExec, RuntimePaths,
    ToolCatalog,
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
        request.argv(&pinned()).map_err(|err| err.to_string())?,
        strings(&[
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "rust[profile=minimal,components=clippy,rustfmt]@1.98.1",
            "--",
            "cargo",
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--no-deps",
            "--manifest-path",
            "/tmp/x y/Cargo.toml",
        ])
    );
    let argv = request.argv(&pinned()).map_err(|err| err.to_string())?;
    assert!(
        !argv.iter().any(|arg| arg == "--offline"),
        "discovery skips dependency resolution through --no-deps"
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
    assert_eq!(
        command.argv(),
        request.argv(&pinned()).map_err(|err| err.to_string())?
    );
    assert_eq!(command.program(), "mise");
    Ok(())
}

#[test]
fn qualification_argv_carries_locked_offline() -> Result<(), String> {
    let request = MetadataQualification::new(PathBuf::from("/repo/Cargo.toml"))
        .map_err(|err| err.to_string())?;
    assert_eq!(
        request.argv(&pinned()).map_err(|err| err.to_string())?,
        strings(&[
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "rust[profile=minimal,components=clippy,rustfmt]@1.98.1",
            "--",
            "cargo",
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--offline",
            "--manifest-path",
            "/repo/Cargo.toml",
        ])
    );
    let argv = request.argv(&pinned()).map_err(|err| err.to_string())?;
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
        request.argv(&pinned()).map_err(|err| err.to_string())?,
        strings(&[
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "rust[profile=minimal,components=clippy,rustfmt]@1.98.1",
            "mr-boxington@1.21.1",
            "--",
            "mbx",
            "clippy",
            "--package",
            "demo",
            "--locked",
        ])
    );
    let argv = request.argv(&pinned()).map_err(|err| err.to_string())?;
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
    let argv = request.argv(&pinned()).map_err(|err| err.to_string())?;
    for spec in [
        "rust[profile=minimal,components=clippy,rustfmt]@1.98.1",
        "mr-boxington@1.21.1",
        "gh@2.102.0",
        "actionlint@1.7.12",
        "shellcheck@0.11.0",
        "zizmor@1.30.1",
        "aqua:nextest-rs/nextest/cargo-nextest@0.9.146",
        "opentofu@1.13.1",
    ] {
        assert!(argv.iter().any(|arg| arg == spec), "missing spec: {spec}");
    }
    Ok(())
}

#[test]
fn pinned_exec_runs_nextest_without_preinstalled_tools() -> Result<(), String> {
    let request = PinnedToolExec::new(
        vec![PinnedTool::Rust, PinnedTool::Nextest],
        OsStr::new("cargo"),
        strings(&[
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
        request.argv(&pinned()).map_err(|err| err.to_string())?,
        strings(&[
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "rust[profile=minimal,components=clippy,rustfmt]@1.98.1",
            "aqua:nextest-rs/nextest/cargo-nextest@0.9.146",
            "--",
            "cargo",
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

#[test]
fn planning_runtime_is_compiled_and_overrides_inherited_mise_root() -> Result<(), String> {
    let request = PinnedToolExec::new(
        vec![PinnedTool::Gh],
        OsStr::new("gh"),
        strings(&["--version"]),
    )
    .map_err(|err| err.to_string())?;
    let command = request
        .command_with_runtime(&pinned(), RuntimePaths::planning())
        .map_err(|err| err.to_string())?;
    let env = command.full_env();
    assert_eq!(
        env.iter()
            .rev()
            .find(|(key, _)| key == "MISE_DATA_DIR")
            .map(|(_, value)| value.to_string_lossy().into_owned()),
        Some("${{ runner.temp }}/velnor/planning/mise".to_owned())
    );
    assert!(format!("{command:?}").contains("Baseline"));
    Ok(())
}

#[test]
fn default_pinned_exec_does_not_select_planning_root() -> Result<(), String> {
    let request = PinnedToolExec::new(
        vec![PinnedTool::Gh],
        OsStr::new("gh"),
        strings(&["--version"]),
    )
    .map_err(|err| err.to_string())?;
    let command = request.command(&pinned()).map_err(|err| err.to_string())?;
    assert!(
        !command
            .full_env()
            .iter()
            .any(|(key, value)| key == "MISE_DATA_DIR"
                && value == "${{ runner.temp }}/velnor/planning/mise")
    );
    Ok(())
}

#[test]
fn planning_install_uses_same_compiled_root_as_planning_exec() -> Result<(), String> {
    let install = velnor_actions_mise::MiseInstall::new(vec![PinnedTool::Gh])
        .map_err(|err| err.to_string())?;
    let command = install
        .command_with_runtime(&pinned(), RuntimePaths::planning())
        .map_err(|err| err.to_string())?;
    assert!(command.full_env().iter().any(|(key, value)| {
        key == "MISE_DATA_DIR" && value == "${{ runner.temp }}/velnor/planning/mise"
    }));
    Ok(())
}

#[test]
fn planning_runtime_rejects_a_later_string_root_override() -> Result<(), String> {
    let request = PinnedToolExec::new(
        vec![PinnedTool::Gh],
        OsStr::new("gh"),
        strings(&["--version"]),
    )
    .map_err(|err| err.to_string())?;
    let command = request
        .command_with_runtime(&pinned(), RuntimePaths::planning())
        .map_err(|err| err.to_string())?;
    let error = command
        .with_env(&[(
            OsString::from("MISE_DATA_DIR"),
            OsString::from("/tmp/foreign"),
        )])
        .expect_err("typed planning root must not be replaced by a string");
    assert!(error.to_string().contains("runtime_path_override"));
    Ok(())
}

#[test]
fn planning_runtime_resolves_concrete_runner_temp_at_spawn_boundary() -> Result<(), String> {
    let request = PinnedToolExec::new(
        vec![PinnedTool::Gh],
        OsStr::new("gh"),
        strings(&["--version"]),
    )
    .map_err(|err| err.to_string())?;
    let planning = request
        .command_with_runtime(&pinned(), RuntimePaths::planning())
        .map_err(|err| err.to_string())?;
    let parent = vec![
        (OsString::from("RUNNER_TEMP"), OsString::from("/runner/tmp")),
        (
            OsString::from("MISE_DATA_DIR"),
            OsString::from("/runner/full/mise"),
        ),
    ];
    let env = planning.spawn_env(&parent);
    let roots: Vec<_> = env
        .iter()
        .filter(|(key, _)| key == "MISE_DATA_DIR")
        .map(|(_, value)| value.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        roots,
        vec![
            "/runner/full/mise".to_owned(),
            "/runner/tmp/velnor/planning/mise".to_owned(),
        ]
    );
    assert!(!roots.iter().any(|root| root.contains("${{")));
    let full = request.command(&pinned()).map_err(|err| err.to_string())?;
    let full_root = full
        .spawn_env(&parent)
        .into_iter()
        .rev()
        .find(|(key, _)| key == "MISE_DATA_DIR")
        .map(|(_, value)| value.to_string_lossy().into_owned());
    assert_eq!(full_root.as_deref(), Some("/runner/full/mise"));
    Ok(())
}

#[test]
fn typed_planning_root_wins_over_an_earlier_owned_home_binding() -> Result<(), String> {
    let request = PinnedToolExec::new(
        vec![PinnedTool::Gh],
        OsStr::new("gh"),
        strings(&["--version"]),
    )
    .map_err(|err| err.to_string())?;
    let command = request
        .command(&pinned())
        .map_err(|err| err.to_string())?
        .with_env(&velnor_actions_mise::command::toolchain_env(
            "/runner/rustup",
            "/runner/cargo",
            "1.98.1",
        ))
        .map_err(|err| err.to_string())?
        .with_runtime_paths(RuntimePaths::planning());
    let root = command
        .full_env()
        .into_iter()
        .rev()
        .find(|(key, _)| key == "MISE_DATA_DIR")
        .map(|(_, value)| value.to_string_lossy().into_owned());
    assert_eq!(
        root.as_deref(),
        Some("${{ runner.temp }}/velnor/planning/mise")
    );
    Ok(())
}

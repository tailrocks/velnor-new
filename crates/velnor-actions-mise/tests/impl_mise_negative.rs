//! Negative and cross-cutting invariant cases: no tool-file reads, no
//! forbidden payloads, catalog-only specs, mise-routed execution.
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use velnor_actions_mise::{
    GitRequest, MetadataDiscovery, MetadataQualification, MiseError, MiseInstall, NextestArchive,
    NextestDriver, PinnedTool, PinnedToolExec, ToolCatalog,
};

fn pinned() -> ToolCatalog {
    ToolCatalog::pinned()
}

fn strings(items: &[&str]) -> Vec<OsString> {
    items.iter().map(OsString::from).collect()
}

#[test]
fn discovery_touches_no_project_mise_files() -> Result<(), String> {
    let request =
        MetadataDiscovery::new(PathBuf::from("/repo/Cargo.toml")).map_err(|err| err.to_string())?;
    for argv in [request.argv(&pinned())] {
        for arg in &argv {
            let text = arg.to_string_lossy();
            for forbidden in ["mise.toml", "mise.lock", ".mise", "mise-version"] {
                assert!(
                    !text.contains(forbidden),
                    "discovery must not reference {forbidden}: {argv:?}"
                );
            }
        }
        assert!(
            argv.iter().any(|arg| arg == "/repo/Cargo.toml"),
            "only the Cargo manifest is addressed: {argv:?}"
        );
    }
    let command = request.command(&pinned()).map_err(|err| err.to_string())?;
    assert_eq!(command.program(), "mise");
    Ok(())
}

#[test]
fn qualification_command_matches_argv() -> Result<(), String> {
    let request = MetadataQualification::new(PathBuf::from("/repo/Cargo.toml"))
        .map_err(|err| err.to_string())?;
    let command = request.command(&pinned()).map_err(|err| err.to_string())?;
    assert_eq!(command.argv(), request.argv(&pinned()));
    assert_eq!(command.program(), "mise");
    Ok(())
}

#[test]
fn forbidden_payloads_are_rejected() {
    for program in [
        "rustup",
        "/root/.cargo/bin/rustup",
        "rustup.exe",
        "bin/rustup",
    ] {
        assert!(
            matches!(
                PinnedToolExec::new(vec![PinnedTool::Rust], OsStr::new(program), Vec::new()),
                Err(MiseError::ForbiddenPayload { .. })
            ),
            "{program} must be rejected"
        );
    }
    assert!(matches!(
        PinnedToolExec::new(
            vec![PinnedTool::Rust],
            OsStr::new("cargo"),
            strings(&["install", "ripgrep"]),
        ),
        Err(MiseError::ForbiddenPayload { .. })
    ));
    assert!(
        PinnedToolExec::new(
            vec![PinnedTool::Rust],
            OsStr::new("cargo"),
            strings(&["metadata", "--format-version", "1"]),
        )
        .is_ok()
    );
    assert!(
        PinnedToolExec::new(
            vec![PinnedTool::Rust, PinnedTool::MrBoxington],
            OsStr::new("mbx"),
            strings(&["build", "--locked"]),
        )
        .is_ok()
    );
}

#[test]
fn mbx_requires_exact_catalog_authority_and_bare_program() {
    for (tools, program) in [
        (vec![PinnedTool::Rust], "mbx"),
        (vec![PinnedTool::Rust, PinnedTool::MrBoxington], "/tmp/mbx"),
        (vec![PinnedTool::Rust, PinnedTool::MrBoxington], "./mbx"),
        (vec![PinnedTool::Rust, PinnedTool::MrBoxington], "mbx.exe"),
    ] {
        let error = PinnedToolExec::new(tools, OsStr::new(program), strings(&["--version"]))
            .expect_err("an ambient or path-qualified MBX executable is not pinned");
        assert!(
            matches!(error, MiseError::ForbiddenPayload { .. }),
            "unexpected error for {program}: {error}"
        );
    }
}

#[test]
fn catalog_mbx_requires_a_pinned_rust_toolchain() {
    let error = PinnedToolExec::new(
        vec![PinnedTool::MrBoxington],
        OsStr::new("mbx"),
        strings(&["+1.99.0", "--version"]),
    )
    .expect_err("catalog MBX execution must have Rust selected through Mise");
    assert!(matches!(error, MiseError::ForbiddenPayload { .. }));
}

#[test]
fn forbidden_rejection_names_program_and_reason() {
    let err = PinnedToolExec::new(
        vec![PinnedTool::Rust],
        OsStr::new("cargo"),
        strings(&["install", "ripgrep"]),
    )
    .expect_err("cargo install must fail");
    assert_eq!(
        err.to_string(),
        "forbidden_payload: cargo: cargo_install_forbidden"
    );
}

#[test]
fn specs_derive_only_from_catalog() -> Result<(), String> {
    let catalog = ToolCatalog::new(
        "1.97.0", "1.18.0", "2.100.0", "1.7.11", "0.10.0", "1.30.0", "0.9.145", "1.13.0",
    )
    .map_err(|err| err.to_string())?;
    let discovery =
        MetadataDiscovery::new(PathBuf::from("/repo/Cargo.toml")).map_err(|err| err.to_string())?;
    assert!(
        discovery
            .argv(&catalog)
            .iter()
            .any(|arg| arg == "rust@1.97.0")
    );
    let exec = PinnedToolExec::new(
        vec![PinnedTool::Rust, PinnedTool::MrBoxington],
        OsStr::new("cargo"),
        strings(&["--version"]),
    )
    .map_err(|err| err.to_string())?;
    let argv = exec.argv(&catalog);
    assert!(argv.iter().any(|arg| arg == "rust@1.97.0"));
    assert!(argv.iter().any(|arg| arg == "mr-boxington@1.18.0"));
    let install = MiseInstall::new(vec![PinnedTool::Nextest]).map_err(|err| err.to_string())?;
    assert!(
        install
            .argv(&catalog)
            .iter()
            .any(|arg| arg == "aqua:nextest-rs/nextest/cargo-nextest@0.9.145")
    );
    Ok(())
}

#[test]
fn every_request_routes_through_mise_or_git() -> Result<(), String> {
    let discovery =
        MetadataDiscovery::new(PathBuf::from("/repo/Cargo.toml")).map_err(|err| err.to_string())?;
    let qualification = MetadataQualification::new(PathBuf::from("/repo/Cargo.toml"))
        .map_err(|err| err.to_string())?;
    let install = MiseInstall::new(vec![PinnedTool::Rust]).map_err(|err| err.to_string())?;
    let exec = PinnedToolExec::new(
        vec![PinnedTool::Rust],
        OsStr::new("cargo"),
        strings(&["--version"]),
    )
    .map_err(|err| err.to_string())?;
    let nextest = NextestArchive::new(NextestDriver::Cargo, "demo", &[], None)
        .map_err(|err| err.to_string())?;
    let catalog = pinned();
    for command in [
        install.command(&pinned()).map_err(|err| err.to_string())?,
        exec.command(&pinned()).map_err(|err| err.to_string())?,
        nextest.command(&pinned()).map_err(|err| err.to_string())?,
    ] {
        assert_eq!(command.program(), "mise");
        assert_eq!(command.argv()[0], OsString::from("mise"));
    }
    for command in [
        discovery.command(&catalog).map_err(|err| err.to_string())?,
        qualification
            .command(&catalog)
            .map_err(|err| err.to_string())?,
    ] {
        assert_eq!(command.program(), "mise");
        assert_eq!(command.argv()[0], OsString::from("mise"));
    }
    let git = GitRequest::rev_parse(vec![OsString::from("--show-toplevel")]).command();
    assert_eq!(git.program(), "git");
    Ok(())
}

#[test]
fn catalog_selected_mbx_build_vector_uses_current_pins() -> Result<(), String> {
    let exec = PinnedToolExec::new(
        vec![PinnedTool::Rust, PinnedTool::MrBoxington],
        OsStr::new("mbx"),
        strings(&[
            "build",
            "--release",
            "--locked",
            "--package",
            "velnor-actions-cli",
            "--bin",
            "velnor-actions",
        ]),
    )
    .map_err(|err| err.to_string())?;
    assert_eq!(
        exec.argv(&pinned()),
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
            "build",
            "--release",
            "--locked",
            "--package",
            "velnor-actions-cli",
            "--bin",
            "velnor-actions",
        ])
    );
    Ok(())
}

#[test]
fn gh_pinned_exec_is_exact() -> Result<(), String> {
    let exec = PinnedToolExec::new(
        vec![PinnedTool::Gh],
        OsStr::new("gh"),
        strings(&[
            "run",
            "list",
            "--workflow",
            "ci.yml",
            "--branch",
            "main",
            "--json",
            "databaseId,headSha,event,conclusion,headBranch",
            "--limit",
            "50",
        ]),
    )
    .map_err(|err| err.to_string())?;
    let argv = exec.argv(&pinned());
    assert_eq!(argv[5], OsString::from("gh@2.102.0"));
    assert_eq!(argv[7], OsString::from("gh"));
    Ok(())
}

#[test]
fn wrapper_preserves_sorted_features_and_target() -> Result<(), String> {
    let payload = strings(&[
        "cargo",
        "test",
        "--locked",
        "--manifest-path",
        "/repo/Cargo.toml",
        "--features",
        "a,b",
        "--target",
        "x86_64-unknown-linux-gnu",
    ]);
    let exec = PinnedToolExec::new(
        vec![PinnedTool::Rust],
        OsStr::new("cargo"),
        payload[1..].to_vec(),
    )
    .map_err(|err| err.to_string())?;
    let argv = exec.argv(&pinned());
    let split = argv.iter().position(|arg| arg == "--").expect("separator");
    assert_eq!(&argv[split + 1..], payload.as_slice());
    Ok(())
}

#[test]
fn toolchain_probe_mechanism_routes_through_pins() -> Result<(), String> {
    for (tools, program) in [
        (vec![PinnedTool::Rust], "cargo"),
        (vec![PinnedTool::Rust], "rustc"),
        (vec![PinnedTool::Rust, PinnedTool::MrBoxington], "mbx"),
    ] {
        let exec = PinnedToolExec::new(tools, OsStr::new(program), strings(&["--version"]))
            .map_err(|err| err.to_string())?;
        let argv = exec.argv(&pinned());
        assert_eq!(argv[0], OsString::from("mise"));
        assert_eq!(argv[4], OsString::from("exec"));
        assert!(
            argv.iter().any(|arg| arg == "rust@1.99.0"),
            "probe must use the exact pin: {argv:?}"
        );
    }
    Ok(())
}

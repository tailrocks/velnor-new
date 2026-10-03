//! Whole-surface invariants: mise-only invocation, no config passthrough,
//! catalog pins without project selectors, and `mise.lock` absence
//! (RQ-2.12, RQ-3.4, RQ-9.3, TOOL-2.6).
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use velnor_actions_mise::catalog::NEXTEST_VERSION;
use velnor_actions_mise::{
    ACTIONLINT_VERSION, BaselineLookup, CandidateBuild, GH_VERSION, IsolatedCommand,
    MR_BOXINGTON_VERSION, MetadataDiscovery, MetadataQualification, MiseInstall, NextestArchive,
    NextestDriver, NextestList, NextestPartition, NextestRun, OPENTOFU_VERSION, PinnedTool,
    PinnedToolExec, PreparePinnedTools, RUST_VERSION, RouteDriver, SHELLCHECK_VERSION,
    TaskCacheMode, TestRunner, ToolCatalog, ToolHomes, VerifyPreparedInputs, VerifySpec,
    VerifyToolchain, ZIZMOR_VERSION, is_allowed_mise_subcommand, qualified_task_run_argv,
};

fn pinned() -> ToolCatalog {
    ToolCatalog::pinned()
}

fn homes() -> Result<ToolHomes, String> {
    ToolHomes::new("/velnor/rustup", "/velnor/cargo").map_err(|err| err.to_string())
}

/// Every mise-owned argv shape, including the program token.
fn all_mise_vectors() -> Result<Vec<Vec<OsString>>, String> {
    let catalog = pinned();
    let specs = vec!["rust@1.98.1".to_owned()];
    let payload = vec![OsString::from("cargo"), OsString::from("--version")];
    let mut vectors = vec![
        IsolatedCommand::mise_exec(&specs, &payload)
            .map_err(|err| err.to_string())?
            .argv(),
        IsolatedCommand::mise_install(&specs)
            .map_err(|err| err.to_string())?
            .argv(),
        PinnedToolExec::new(
            vec![PinnedTool::Rust],
            OsStr::new("cargo"),
            payload[1..].to_vec(),
        )
        .map_err(|err| err.to_string())?
        .argv(&catalog),
        MiseInstall::new(vec![PinnedTool::Rust])
            .map_err(|err| err.to_string())?
            .argv(&catalog),
        MetadataDiscovery::new(PathBuf::from("demo/Cargo.toml"))
            .map_err(|err| err.to_string())?
            .argv(&catalog),
        MetadataQualification::new(PathBuf::from("Cargo.toml"))
            .map_err(|err| err.to_string())?
            .argv(&catalog),
        PreparePinnedTools::new(vec![PinnedTool::Rust], homes()?)
            .map_err(|err| err.to_string())?
            .argv(&catalog),
        VerifyPreparedInputs::new(PathBuf::from("Cargo.toml"), homes()?)
            .map_err(|err| err.to_string())?
            .argv(&catalog),
        CandidateBuild::new()
            .map_err(|err| err.to_string())?
            .argv(&catalog),
    ];
    vectors.extend(nextest_vectors(&catalog)?);
    let verify = VerifyToolchain::new(
        &catalog,
        VerifySpec {
            driver: RouteDriver::Cargo,
            runner: TestRunner::CargoNextest,
            format: "cargo-1",
            generation: "gen-7",
            target: "host",
            platform: "ubuntu-26.04",
            homes: homes()?,
            findings: Vec::new(),
        },
    )
    .map_err(|err| err.to_string())?;
    vectors.extend(verify.probes(&catalog));
    let lookup = BaselineLookup::new(
        "0123456789abcdef0123456789abcdef01234567",
        "ci.yml",
        "main",
        "coverage-manifests",
    )
    .map_err(|err| err.to_string())?;
    vectors.push(lookup.list_argv(&catalog).map_err(|err| err.to_string())?);
    vectors.push(
        lookup
            .download_argv(&catalog, 7, &PathBuf::from("/tmp/velnor-base"))
            .map_err(|err| err.to_string())?,
    );
    let gated = qualified_task_run_argv(
        "test",
        false,
        false,
        false,
        TaskCacheMode::ReadWrite,
        "clippy",
        "$RUNNER_TEMP/velnor/tasks/clippy.toml",
    )
    .map_err(|err| err.to_string())?;
    vectors.push(gated.iter().map(OsString::from).collect());
    Ok(vectors)
}

/// Nextest archive/list/run argv shapes.
fn nextest_vectors(catalog: &ToolCatalog) -> Result<Vec<Vec<OsString>>, String> {
    let partition = NextestPartition::new(1, 1).map_err(|err| err.to_string())?;
    Ok(vec![
        NextestArchive::new(NextestDriver::Cargo, "demo", &[], None)
            .map_err(|err| err.to_string())?
            .argv(catalog),
        NextestList::new(NextestDriver::Cargo, partition).argv(catalog),
        NextestRun::new(NextestDriver::Cargo, partition, "m-abc", "p1")
            .map_err(|err| err.to_string())?
            .argv(catalog),
    ])
}

#[test]
fn all_mise_vectors_invoke_mise_program() -> Result<(), String> {
    let vectors = all_mise_vectors()?;
    assert!(!vectors.is_empty());
    for argv in &vectors {
        assert_eq!(argv[0], OsString::from("mise"), "mise-only: {argv:?}");
        let subcommand = argv
            .iter()
            .skip(1)
            .find(|arg| !arg.to_string_lossy().starts_with("--"))
            .map(|arg| arg.to_string_lossy().into_owned())
            .ok_or_else(|| format!("missing subcommand: {argv:?}"))?;
        assert!(
            is_allowed_mise_subcommand(&subcommand),
            "allowlisted subcommand: {subcommand}"
        );
    }
    Ok(())
}

#[test]
fn no_config_passthrough_in_vectors() -> Result<(), String> {
    let tokens = [
        "mise.toml",
        "mise.lock",
        ".tool-versions",
        "tool-versions",
        "--config",
        "config.toml",
        "rust-toolchain.toml",
    ];
    for argv in all_mise_vectors()? {
        for arg in &argv {
            let text = arg.to_string_lossy();
            for token in tokens {
                assert!(
                    !text.contains(token),
                    "config passthrough `{token}` in {argv:?}"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn catalog_pins_ignore_project_selectors() -> Result<(), String> {
    let catalog = ToolCatalog::new(
        RUST_VERSION,
        MR_BOXINGTON_VERSION,
        GH_VERSION,
        ACTIONLINT_VERSION,
        SHELLCHECK_VERSION,
        ZIZMOR_VERSION,
        NEXTEST_VERSION,
        OPENTOFU_VERSION,
    )
    .map_err(|err| err.to_string())?;
    assert_eq!(catalog, ToolCatalog::pinned());
    let first = CandidateBuild::new().map_err(|err| err.to_string())?;
    let second = CandidateBuild::new().map_err(|err| err.to_string())?;
    assert_eq!(first.argv(&pinned()), second.argv(&pinned()));
    assert!(
        first
            .argv(&pinned())
            .iter()
            .any(|arg| arg == "mr-boxington@1.21.1"),
        "exact MBX invocation never downgrades"
    );
    Ok(())
}

#[test]
fn mise_lock_neither_present_nor_tracked() -> Result<(), String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map(std::path::Path::to_path_buf)
        .ok_or_else(|| "crate is not two levels below the workspace root".to_owned())?;
    assert!(
        !root.join("mise.lock").exists(),
        "Velnor must not create mise.lock"
    );
    let tracked = std::process::Command::new("git")
        .args(["ls-files", "mise.lock"])
        .current_dir(&root)
        .output();
    if let Ok(output) = tracked {
        assert!(output.stdout.is_empty(), "mise.lock stays untracked");
    }
    Ok(())
}

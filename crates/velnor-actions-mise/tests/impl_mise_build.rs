//! Candidate-build vector cases (BOOT-4.8).
use std::ffi::OsString;
use velnor_actions_mise::{
    CANDIDATE_BUILD_BIN, CANDIDATE_BUILD_PACKAGE, CandidateBuild, PinnedTool, ToolCatalog,
};

fn pinned() -> ToolCatalog {
    ToolCatalog::pinned()
}

fn strings(items: &[&str]) -> Vec<OsString> {
    items.iter().map(OsString::from).collect()
}

#[test]
fn candidate_build_argv_is_byte_exact() -> Result<(), String> {
    assert_eq!(CANDIDATE_BUILD_PACKAGE, "velnor-actions-cli");
    assert_eq!(CANDIDATE_BUILD_BIN, "velnor-actions");
    let build = CandidateBuild::new().map_err(|err| err.to_string())?;
    assert_eq!(
        build.argv(&pinned()),
        strings(&[
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "rust@1.98.1",
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
fn candidate_build_command_matches_argv() -> Result<(), String> {
    let catalog = pinned();
    let build = CandidateBuild::new().map_err(|err| err.to_string())?;
    let command = build.command(&catalog).map_err(|err| err.to_string())?;
    assert_eq!(command.program(), "mise");
    assert_eq!(command.argv(), build.argv(&catalog));
    assert!(
        command.disables_auto_install(),
        "candidate build must not fetch tools: {command:?}"
    );
    Ok(())
}

#[test]
fn candidate_build_specs_come_only_from_catalog() -> Result<(), String> {
    let catalog = ToolCatalog::new(
        "1.97.0", "1.18.0", "2.100.0", "1.7.11", "0.10.0", "1.30.0", "0.9.145", "1.13.0",
    )
    .map_err(|err| err.to_string())?;
    let build = CandidateBuild::new().map_err(|err| err.to_string())?;
    let argv = build.argv(&catalog);
    assert!(argv.iter().any(|arg| arg == "rust@1.97.0"));
    assert_eq!(
        catalog.tool_spec(PinnedTool::MrBoxington),
        "mr-boxington@1.18.0"
    );
    assert!(
        !argv
            .iter()
            .any(|arg| arg.to_string_lossy().starts_with("mr-boxington@")),
        "native action owns MBX; candidate Mise selectors exclude it: {argv:?}"
    );
    assert!(
        !argv.iter().any(|arg| arg == "mr-boxington@1.21.1"),
        "no pinned fallback may leak in: {argv:?}"
    );
    Ok(())
}

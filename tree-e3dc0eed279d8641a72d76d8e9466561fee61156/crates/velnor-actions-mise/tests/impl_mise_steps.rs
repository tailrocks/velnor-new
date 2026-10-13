//! `Prepare pinned tools` step cases (TASK-2.1, WF-3.39).
use std::ffi::OsString;
use velnor_actions_mise::{
    MiseError, PREPARE_PINNED_TOOLS_STEP, PinnedTool, PreparePinnedTools, ToolCatalog, ToolHomes,
};

fn pinned() -> ToolCatalog {
    ToolCatalog::pinned()
}

fn homes() -> Result<ToolHomes, String> {
    ToolHomes::new("/velnor/rustup", "/velnor/cargo").map_err(|err| err.to_string())
}

fn prepare() -> Result<PreparePinnedTools, String> {
    PreparePinnedTools::new(vec![PinnedTool::Rust, PinnedTool::MrBoxington], homes()?)
        .map_err(|err| err.to_string())
}

fn strings(items: &[&str]) -> Vec<OsString> {
    items.iter().map(OsString::from).collect()
}

fn env_has(env: &[(OsString, OsString)], key: &str, value: &str) -> bool {
    env.iter()
        .any(|(item_key, item_value)| item_key == key && item_value == value)
}

#[test]
fn prepare_step_name_matches_both_contracts() {
    assert_eq!(PREPARE_PINNED_TOOLS_STEP, "Prepare pinned tools");
    assert_eq!(PreparePinnedTools::step_name(), PREPARE_PINNED_TOOLS_STEP);
    assert_eq!(PreparePinnedTools::step_name(), "Prepare pinned tools");
}

#[test]
fn prepare_pinned_tools_argv_is_fixed_install() -> Result<(), String> {
    assert_eq!(
        prepare()?.argv(&pinned()),
        strings(&[
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "install",
            "rust@1.98.1",
            "mr-boxington@1.19.0",
        ])
    );
    Ok(())
}

#[test]
fn prepare_pinned_tools_env_disables_knobs_and_carries_homes() -> Result<(), String> {
    let env = prepare()?.env(&pinned());
    for (key, value) in [
        ("MISE_NO_CONFIG", "1"),
        ("MISE_NO_ENV", "1"),
        ("MISE_NO_HOOKS", "1"),
        ("MISE_LOCKFILE", "0"),
        ("MISE_RUSTUP_HOME", "/velnor/rustup"),
        ("MISE_CARGO_HOME", "/velnor/cargo"),
        ("RUSTUP_TOOLCHAIN", "1.98.1"),
    ] {
        assert!(env_has(&env, key, value), "missing {key}={value}: {env:?}");
    }
    for blocked in ["MISE_AUTO_INSTALL", "MISE_EXEC_AUTO_INSTALL"] {
        assert!(
            !env.iter().any(|(key, _)| key == blocked),
            "explicit install must stay enabled: {blocked}"
        );
    }
    assert_eq!(env.len(), 7, "exact step env, no drift: {env:?}");
    Ok(())
}

#[test]
fn prepare_pinned_tools_command_matches_step() -> Result<(), String> {
    let step = prepare()?;
    let command = step.command(&pinned()).map_err(|err| err.to_string())?;
    assert_eq!(command.program(), "mise");
    assert_eq!(command.argv(), step.argv(&pinned()));
    assert_eq!(command.full_env(), step.env(&pinned()));
    Ok(())
}

#[test]
fn prepare_pinned_tools_rejects_empty_toolchain() -> Result<(), String> {
    assert!(matches!(
        PreparePinnedTools::new(Vec::new(), homes()?),
        Err(MiseError::EmptyToolchain)
    ));
    Ok(())
}

#[test]
fn prepare_pinned_tools_specs_come_only_from_catalog() -> Result<(), String> {
    let catalog = ToolCatalog::new(
        "1.97.0", "1.18.0", "2.100.0", "1.7.11", "0.10.0", "1.30.0", "0.9.145",
    )
    .map_err(|err| err.to_string())?;
    let step =
        PreparePinnedTools::new(vec![PinnedTool::Rust], homes()?).map_err(|err| err.to_string())?;
    let argv = step.argv(&catalog);
    assert!(argv.iter().any(|arg| arg == "rust@1.97.0"));
    assert!(
        !argv.iter().any(|arg| arg == "rust@1.98.1"),
        "no pinned fallback may leak in: {argv:?}"
    );
    assert!(env_has(&step.env(&catalog), "RUSTUP_TOOLCHAIN", "1.97.0"));
    Ok(())
}

#[test]
fn prepare_pinned_tools_exposes_tools_and_homes() -> Result<(), String> {
    let step = prepare()?;
    assert_eq!(step.tools(), &[PinnedTool::Rust, PinnedTool::MrBoxington]);
    assert_eq!(step.homes().rustup_home(), "/velnor/rustup");
    assert_eq!(step.homes().cargo_home(), "/velnor/cargo");
    Ok(())
}

#[test]
fn tool_homes_rejects_empty() {
    assert!(matches!(
        ToolHomes::new("", "/velnor/cargo"),
        Err(MiseError::InvalidStepInput { .. })
    ));
    assert!(matches!(
        ToolHomes::new("/velnor/rustup", ""),
        Err(MiseError::InvalidStepInput { .. })
    ));
    let err = ToolHomes::new("", "").expect_err("empty homes must fail");
    assert_eq!(err.to_string(), "invalid_step_input: rustup_home: ");
}

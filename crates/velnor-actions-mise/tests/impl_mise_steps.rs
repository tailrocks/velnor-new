//! `Prepare pinned tools` and `Verify prepared inputs` step cases (TASK-2.1, TASK-2.5, WF-3.39).
use std::ffi::OsString;
use std::path::PathBuf;
use velnor_actions_mise::{
    MiseError, PREPARE_PINNED_TOOLS_STEP, PREPARE_RUST_COMPONENTS_STEP, PREPARE_RUST_TARGET_STEP,
    PinnedTool, PreparePinnedTools, PrepareRustComponents, PrepareRustTarget, ToolCatalog,
    ToolHomes, VERIFY_PREPARED_INPUTS_STEP, VerifyPreparedInputs,
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
            "rust@1.99.0",
            "mr-boxington@1.23.0",
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
        ("RUSTUP_TOOLCHAIN", "1.99.0"),
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
fn prepare_pinned_tools_env_without_homes_is_isolation_only() -> Result<(), String> {
    let step = prepare()?;
    let env = step.env_without_homes();
    let expected: Vec<(OsString, OsString)> = [
        ("MISE_NO_CONFIG", "1"),
        ("MISE_NO_ENV", "1"),
        ("MISE_NO_HOOKS", "1"),
        ("MISE_LOCKFILE", "0"),
    ]
    .into_iter()
    .map(|(key, value)| (OsString::from(key), OsString::from(value)))
    .collect();
    assert_eq!(
        env, expected,
        "triple-less prepare env is the exact isolation overlay: {env:?}"
    );
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
        "1.97.0", "1.18.0", "2.100.0", "1.7.11", "0.10.0", "1.30.0", "0.9.145", "1.13.0",
    )
    .map_err(|err| err.to_string())?;
    let step =
        PreparePinnedTools::new(vec![PinnedTool::Rust], homes()?).map_err(|err| err.to_string())?;
    let argv = step.argv(&catalog);
    assert!(argv.iter().any(|arg| arg == "rust@1.97.0"));
    assert!(
        !argv.iter().any(|arg| arg == "rust@1.99.0"),
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
fn prepared_inputs_step_name_is_contract_fixed() {
    assert_eq!(VERIFY_PREPARED_INPUTS_STEP, "Verify prepared inputs");
    assert_eq!(
        VerifyPreparedInputs::step_name(),
        VERIFY_PREPARED_INPUTS_STEP
    );
}

#[test]
fn prepared_inputs_argv_is_locked_offline_qualification() -> Result<(), String> {
    let step = VerifyPreparedInputs::new(PathBuf::from("Cargo.toml"), homes()?)
        .map_err(|err| err.to_string())?;
    assert_eq!(
        step.workspace_manifest(),
        PathBuf::from("Cargo.toml").as_path()
    );
    assert_eq!(step.homes().rustup_home(), "/velnor/rustup");
    assert_eq!(
        step.argv(&pinned()),
        strings(&[
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "rust@1.99.0",
            "--",
            "mbx",
            "+1.99.0",
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--offline",
            "--manifest-path",
            "Cargo.toml",
        ])
    );
    Ok(())
}

#[test]
fn prepared_inputs_env_matches_command() -> Result<(), String> {
    let catalog = pinned();
    let step = VerifyPreparedInputs::new(PathBuf::from("Cargo.toml"), homes()?)
        .map_err(|err| err.to_string())?;
    let command = step.command(&catalog).map_err(|err| err.to_string())?;
    assert_eq!(command.program(), "mise");
    assert_eq!(command.argv(), step.argv(&catalog));
    assert_eq!(command.full_env(), step.env(&catalog));
    assert!(command.disables_auto_install());
    let env = step.env(&catalog);
    for (key, value) in [
        ("MISE_NO_CONFIG", "1"),
        ("MISE_LOCKFILE", "0"),
        ("MISE_AUTO_INSTALL", "false"),
        ("MISE_EXEC_AUTO_INSTALL", "false"),
        ("MISE_RUSTUP_HOME", "/velnor/rustup"),
        ("MISE_CARGO_HOME", "/velnor/cargo"),
        ("RUSTUP_TOOLCHAIN", "1.99.0"),
    ] {
        assert!(env_has(&env, key, value), "missing {key}={value}: {env:?}");
    }
    assert_eq!(env.len(), 9, "exact step env, no drift: {env:?}");
    Ok(())
}

#[test]
fn prepared_inputs_rejects_empty_manifest() -> Result<(), String> {
    assert!(matches!(
        VerifyPreparedInputs::new(PathBuf::from(""), homes()?),
        Err(MiseError::InvalidManifestPath { .. })
    ));
    Ok(())
}

#[test]
fn tool_homes_runner_temp_uses_expression_paths() {
    let homes = ToolHomes::runner_temp();
    assert_eq!(
        homes.rustup_home(),
        "${{ runner.temp }}/velnor/rustup",
        "shell $VAR never expands in env position"
    );
    assert_eq!(
        homes.cargo_home(),
        "${{ runner.temp }}/velnor/cargo",
        "shell $VAR never expands in env position"
    );
}

#[test]
fn tool_homes_exec_env_is_verification_env() -> Result<(), String> {
    let catalog = pinned();
    let env = homes()?.exec_env(&catalog);
    for (key, value) in [
        ("MISE_NO_CONFIG", "1"),
        ("MISE_NO_ENV", "1"),
        ("MISE_NO_HOOKS", "1"),
        ("MISE_LOCKFILE", "0"),
        ("MISE_AUTO_INSTALL", "false"),
        ("MISE_EXEC_AUTO_INSTALL", "false"),
        ("MISE_RUSTUP_HOME", "/velnor/rustup"),
        ("MISE_CARGO_HOME", "/velnor/cargo"),
        ("RUSTUP_TOOLCHAIN", "1.99.0"),
    ] {
        assert!(env_has(&env, key, value), "missing {key}={value}: {env:?}");
    }
    assert_eq!(env.len(), 9, "exact step env, no drift: {env:?}");
    let qualified = VerifyPreparedInputs::new(PathBuf::from("Cargo.toml"), homes()?)
        .map_err(|err| err.to_string())?;
    assert_eq!(
        env,
        qualified.env(&catalog),
        "one exec-env constructor serves every verification step"
    );
    Ok(())
}

#[test]
fn rust_components_argv_is_fixed_rustup_add() -> Result<(), String> {
    assert_eq!(PREPARE_RUST_COMPONENTS_STEP, "Prepare Rust components");
    let request = PrepareRustComponents::new(homes()?);
    assert_eq!(
        PrepareRustComponents::step_name(),
        PREPARE_RUST_COMPONENTS_STEP
    );
    assert_eq!(PrepareRustComponents::components(), ["clippy", "rustfmt"]);
    assert_eq!(
        request.argv(&pinned()),
        strings(&[
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "rust@1.99.0",
            "--",
            "rustup",
            "component",
            "add",
            "--toolchain",
            "1.99.0-x86_64-unknown-linux-gnu",
            "clippy",
            "rustfmt",
        ])
    );
    Ok(())
}

#[test]
fn rust_components_command_matches_step() -> Result<(), String> {
    let catalog = pinned();
    let request = PrepareRustComponents::new(homes()?);
    let command = request.command(&catalog).map_err(|err| err.to_string())?;
    let env = request.env(&catalog);
    assert_eq!(command.argv(), request.argv(&catalog));
    assert_eq!(command.full_env(), env);
    assert!(command.disables_auto_install());
    assert!(env_has(&env, "RUSTUP_TOOLCHAIN", "1.99.0"));
    assert_eq!(
        env_value(&env, "RUSTUP_HOME"),
        env_value(&env, "MISE_RUSTUP_HOME")
    );
    assert_eq!(
        env_value(&env, "CARGO_HOME"),
        env_value(&env, "MISE_CARGO_HOME")
    );
    assert!(env_value(&env, "RUSTUP_HOME").is_some());
    Ok(())
}

fn env_value<'a>(env: &'a [(OsString, OsString)], key: &str) -> Option<&'a OsString> {
    env.iter()
        .find(|(item_key, _)| item_key == key)
        .map(|(_, value)| value)
}

#[test]
fn rust_target_step_name_matches_contract() {
    assert_eq!(PREPARE_RUST_TARGET_STEP, "Prepare Rust target");
    assert_eq!(PrepareRustTarget::step_name(), PREPARE_RUST_TARGET_STEP);
}

#[test]
fn rust_target_argv_pins_host_toolchain_and_target() -> Result<(), String> {
    let request = PrepareRustTarget::new("aarch64-apple-darwin", "x86_64-apple-darwin")
        .map_err(|err| err.to_string())?;
    assert_eq!(request.host(), "aarch64-apple-darwin");
    assert_eq!(request.target(), "x86_64-apple-darwin");
    assert_eq!(
        request.argv(&pinned()),
        strings(&[
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "rust@1.99.0",
            "--",
            "rustup",
            "target",
            "add",
            "--toolchain",
            "1.99.0-aarch64-apple-darwin",
            "x86_64-apple-darwin",
        ])
    );
    Ok(())
}

#[test]
fn rust_target_rejects_empty_triple() {
    assert!(matches!(
        PrepareRustTarget::new("", "x86_64-apple-darwin"),
        Err(MiseError::InvalidStepInput { .. })
    ));
    assert!(matches!(
        PrepareRustTarget::new("aarch64-apple-darwin", ""),
        Err(MiseError::InvalidStepInput { .. })
    ));
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

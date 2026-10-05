use super::*;

fn run_product_command(stage: &Stage) -> Result<(), String> {
    let command = IsolatedCommand::mise_exec(&[], &[OsString::from("/bin/true")])
        .map_err(|err| err.to_string())?
        .with_cwd(stage.root.clone())
        .with_env(&[
            (OsString::from("HOME"), stage.home.clone().into_os_string()),
            (
                OsString::from("XDG_CONFIG_HOME"),
                stage.xdg.clone().into_os_string(),
            ),
            (OsString::from("PATH"), stage.bin.clone().into_os_string()),
        ])
        .map_err(|err| err.to_string())?;
    let argv = command.argv();
    assert_eq!(&argv[1..4], &MISE_GLOBAL_FLAGS.map(OsString::from));
    for (key, value) in [
        ("MISE_NO_CONFIG", "1"),
        ("MISE_NO_ENV", "1"),
        ("MISE_NO_HOOKS", "1"),
    ] {
        assert!(
            command
                .full_env()
                .iter()
                .any(|(found, seen)| found == key && seen == value)
        );
    }
    let output = command
        .run_bounded(1024 * 1024, Duration::from_secs(60))
        .map_err(|err| err.to_string())?;
    assert!(
        output.success,
        "IsolatedCommand: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

fn launch_product_test(stage: &Stage, binary: &Path) -> Result<Output, String> {
    let current = std::env::current_exe().map_err(|err| err.to_string())?;
    Command::new(current)
        .args([
            "--exact",
            "impl_miserc_isolation::product::production_command_keeps_no_config_guards_with_a_real_binary",
            "--ignored",
            "--nocapture",
        ])
        .env_clear()
        .env(FIXED_BINARY_ENV, binary)
        .env(
            PRODUCT_STAGE_ENV,
            stage.root.parent().ok_or("stage root has no parent")?,
        )
        .env(PRODUCT_STAGE_TOKEN_ENV, &stage.marker)
        .env("HOME", &stage.home)
        .env("XDG_CONFIG_HOME", &stage.xdg)
        .env("MISE_SYSTEM_CONFIG_DIR", &stage.system)
        .env("PATH", &stage.bin)
        .stdin(Stdio::null())
        .output()
        .map_err(|err| format!("cannot run nested test harness: {err}"))
}

#[test]
#[ignore = "official mise releases are selected by the mandatory real-binary qualification lane"]
fn production_command_keeps_no_config_guards_with_a_real_binary() -> Result<(), String> {
    let binary = official_binary(FIXED_BINARY_ENV, FIXED_RELEASE, FIXED_SOURCE)?;
    if let Some(base) = std::env::var_os(PRODUCT_STAGE_ENV) {
        let marker = std::env::var(PRODUCT_STAGE_TOKEN_ENV)
            .map_err(|err| format!("missing child fixture token: {err}"))?;
        return run_product_command(&Stage::from_child_base(&PathBuf::from(base), marker)?);
    }
    let stage = Stage::new()?;
    stage.write_bad_layers(&[Layer::Project, Layer::Global, Layer::System])?;
    symlink(&binary, stage.bin.join("mise")).map_err(|err| err.to_string())?;
    let output = launch_product_test(&stage, &binary)?;
    assert!(
        output.status.success(),
        "nested production-boundary test failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output_text(&output).contains(
            "impl_miserc_isolation::product::production_command_keeps_no_config_guards_with_a_real_binary ... ok"
        ),
        "nested test harness did not report the targeted test: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[test]
fn real_mise_miserc_isolation_handles_the_current_release() -> Result<(), String> {
    let binary = MiseBinary::select()?;
    let (production_args, production_env) = production_invocation()?;
    eprintln!("mise binary path={}", binary.path.display());
    for layer in [
        ConfigLayer::Project,
        ConfigLayer::Global,
        ConfigLayer::System,
    ] {
        let fixture = Fixture::new(Some(layer))?;
        let layer_name = match layer {
            ConfigLayer::Project => "project",
            ConfigLayer::Global => "global",
            ConfigLayer::System => "system",
        };
        for nested in [false, true] {
            let cwd = fixture.cwd(nested);
            let control = run_raw(
                &binary.path,
                &fixture,
                cwd,
                &[OsString::from("--version")],
                &[],
            )?;
            assert_miserc_error(&control, &fixture)
                .map_err(|err| format!("control layer={layer_name} nested={nested}: {err}"))?;
            for selection in [Selection::FlagOnly, Selection::EnvironmentOnly] {
                for operation in [Operation::Version, Operation::Exec] {
                    let overlay = selected_env(selection);
                    let args = selected_args(selection, operation);
                    let has_flag = args.iter().any(|arg| arg == "--no-config");
                    let has_env = overlay
                        .iter()
                        .any(|(key, value)| key == "MISE_NO_CONFIG" && value == "1");
                    if has_flag != matches!(selection, Selection::FlagOnly)
                        || has_env != matches!(selection, Selection::EnvironmentOnly)
                    {
                        return Err("no-config selectors were not isolated".to_owned());
                    }
                    let output = run_raw(&binary.path, &fixture, cwd, &args, &overlay)?;
                    assert_no_config_behavior(&binary, &output, &fixture, operation).map_err(
                        |err| {
                            format!(
                                "no-config layer={layer_name} nested={nested} selection={} operation={}: {err}",
                                if matches!(selection, Selection::FlagOnly) { "flag" } else { "env" },
                                if matches!(operation, Operation::Version) { "version" } else { "exec" },
                            )
                        },
                    )?;
                }
            }
            let output = run_raw(
                &binary.path,
                &fixture,
                cwd,
                &production_args,
                &production_env,
            )?;
            assert_no_config_behavior(&binary, &output, &fixture, Operation::Exec)
                .map_err(|err| format!("production layer={layer_name} nested={nested}: {err}"))?;
        }
    }
    Ok(())
}

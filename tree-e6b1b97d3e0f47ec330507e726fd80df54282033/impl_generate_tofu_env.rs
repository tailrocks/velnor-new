//! Tofu env parity: key consts agree across crates, the renderer's
//! TF-family predicate agrees with the Mise reserved rule, and the
//! isolation pairs agree between author and spawn constructor.
use std::ffi::OsString;

#[test]
fn tofu_key_consts_agree_across_crates() {
    assert_eq!(
        velnor_actions_mise::TF_IN_AUTOMATION_ENV,
        velnor_actions_tofu::TF_IN_AUTOMATION_ENV
    );
    assert_eq!(
        velnor_actions_mise::TF_IN_AUTOMATION_ON,
        velnor_actions_tofu::TF_IN_AUTOMATION_ON
    );
    assert_eq!(
        velnor_actions_mise::TF_INPUT_ENV,
        velnor_actions_tofu::TF_INPUT_ENV
    );
    assert_eq!(
        velnor_actions_mise::TF_INPUT_OFF,
        velnor_actions_tofu::TF_INPUT_OFF
    );
    assert_eq!(
        velnor_actions_mise::TF_DATA_DIR_ENV,
        velnor_actions_tofu::TF_DATA_DIR_ENV
    );
    assert_eq!(
        velnor_actions_mise::TF_CLI_CONFIG_FILE_ENV,
        velnor_actions_tofu::TF_CLI_CONFIG_FILE_ENV
    );
    assert_eq!(
        velnor_actions_mise::TF_PLUGIN_CACHE_DIR_ENV,
        velnor_actions_tofu::TF_PLUGIN_CACHE_DIR_ENV
    );
    assert_eq!(
        velnor_actions_mise::runtime_paths::TOFU_DATA_BASE_EXPR,
        "${{ runner.temp }}/velnor/tofu-data"
    );
}

#[test]
fn tf_predicate_parity_with_documented_divergence() {
    for key in [
        "TF_VAR_secret",
        "TF_CLI_ARGS_plan",
        "TF_TOKEN_app",
        "TF_DATA_DIR",
        "TF_CLI_CONFIG_FILE",
        "TF_PLUGIN_CACHE_DIR",
        "TF_WORKSPACE",
        "TF_LOG",
        "TF_LOG_PATH",
        "TF_REGISTRY_CLIENT_TIMEOUT",
        "CHECKPOINT_DISABLE",
        "TF_IN_AUTOMATION",
        "TF_INPUT",
        "MISE_RUSTUP_HOME",
        "VELNOR_TASK_RUN",
        "PATH",
    ] {
        assert_eq!(
            velnor_actions_workflow_renderer::toolchain_env::is_denied_tf_key(key),
            velnor_actions_mise::command::is_reserved_env_key(key),
            "TF-family parity for {key}"
        );
    }
    // Deliberate divergence: `TOFU_*` has no tool reader and no wire
    // presence, so only the Mise reserved rule (the spawn side) holds
    // the future family; the renderer leaves it alone.
    assert!(
        velnor_actions_mise::command::is_reserved_env_key("TOFU_FUTURE_KEY"),
        "mise must reserve the future family"
    );
    assert!(
        !velnor_actions_workflow_renderer::toolchain_env::is_denied_tf_key("TOFU_FUTURE_KEY"),
        "renderer must leave the future family alone"
    );
}

#[test]
fn isolation_pairs_agree_between_author_and_constructor() -> Result<(), String> {
    let authored = velnor_actions_tofu::tofu_isolation_env(
        "/velnor/tofu-data",
        "/velnor/tofu-cli.hcl",
        "/velnor/tofu-cache",
    );
    let command = velnor_actions_mise::IsolatedCommand::tofu_exec(
        &["opentofu@1.13.1".to_owned()],
        &[OsString::from("tofu"), OsString::from("version")],
        "/velnor/tofu-data",
        "/velnor/tofu-cli.hcl",
        "/velnor/tofu-cache",
    )
    .map_err(|err| err.to_string())?;
    let full = command.full_env();
    assert_eq!(&full[full.len() - authored.len()..], authored.as_slice());
    Ok(())
}

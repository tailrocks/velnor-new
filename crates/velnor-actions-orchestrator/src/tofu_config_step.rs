//! Materialized CLI configuration and cleared child environment for Tofu.

use std::collections::BTreeMap;

use velnor_actions_contract::Step;

use crate::OrchestratorError;

const HOME_BASE: &str = "${{ runner.temp }}/velnor/tofu-home";

/// Exact generator-owned paths and policy for one normalized root.
pub(crate) fn task_env(root: &str) -> Result<BTreeMap<String, String>, OrchestratorError> {
    velnor_actions_tofu::validate_normalized_root(root)?;
    let home = format!(
        "{HOME_BASE}/{}",
        velnor_actions_tofu::tofu_root_locator(root)?
    );
    let data = velnor_actions_tofu::tofu_data_dir_under(
        velnor_actions_mise::runtime_paths::TOFU_DATA_BASE_EXPR,
        root,
    )
    .map_err(contract_error)?;
    let cache = crate::tofu_cache::tofu_provider_cache_path(root)?;
    let mut env = BTreeMap::from([
        ("HOME".to_owned(), home.clone()),
        ("XDG_CONFIG_HOME".to_owned(), format!("{home}/config")),
        ("XDG_DATA_HOME".to_owned(), format!("{home}/data")),
        ("XDG_CACHE_HOME".to_owned(), format!("{home}/cache")),
        ("TF_CLI_CONFIG_FILE".to_owned(), format!("{home}/cli.tfrc")),
        ("TF_DATA_DIR".to_owned(), data),
        ("TF_PLUGIN_CACHE_DIR".to_owned(), cache),
        ("TF_IN_AUTOMATION".to_owned(), "1".to_owned()),
        ("TF_INPUT".to_owned(), "0".to_owned()),
    ]);
    for (key, value) in velnor_actions_mise::ISOLATION_ENV
        .iter()
        .chain(velnor_actions_mise::NO_AUTO_INSTALL_ENV.iter())
    {
        env.insert((*key).to_owned(), (*value).to_owned());
    }
    Ok(env)
}

fn contract_error(error: velnor_actions_contract::ContractError) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: error.to_string(),
    }
}

/// Clear all ambient selectors/credentials before executing the task payload.
/// Report helpers remain outside this prefix and keep their identity variables.
pub(crate) fn isolation_prefix() -> String {
    let mut prefix = String::from("env -i PATH=\"$PATH\" MISE_DATA_DIR=\"$MISE_DATA_DIR\"");
    for key in [
        "HOME",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "TF_CLI_CONFIG_FILE",
        "TF_DATA_DIR",
        "TF_PLUGIN_CACHE_DIR",
        "TF_IN_AUTOMATION",
        "TF_INPUT",
        "MISE_NO_CONFIG",
        "MISE_NO_ENV",
        "MISE_NO_HOOKS",
        "MISE_LOCKFILE",
        "MISE_AUTO_INSTALL",
        "MISE_EXEC_AUTO_INSTALL",
    ] {
        prefix.push_str(&format!(" {key}=\"${key}\""));
    }
    prefix.push(' ');
    prefix
}

/// Create separate mutable paths and a config with no provider overrides.
pub(crate) fn materialization_step(root: &str) -> Result<Step, OrchestratorError> {
    let script = format!(
        "{}{}",
        include_str!("tofu_root_owner.sh"),
        r#"case "$TF_PLUGIN_CACHE_DIR" in *[!a-zA-Z0-9_./-]*) echo 'tofu_config_unsafe_path' >&2; exit 1;; esac
for path in "$HOME" "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_CACHE_HOME" "$TF_DATA_DIR" "$TF_PLUGIN_CACHE_DIR" "$TF_CLI_CONFIG_FILE"; do check_path "$path"; done
own_directory "$HOME"
own_directory "$TF_DATA_DIR"
mkdir -p "$HOME" "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_CACHE_HOME" "$TF_DATA_DIR" "$TF_PLUGIN_CACHE_DIR"
for path in "$HOME" "$TF_DATA_DIR" "$TF_PLUGIN_CACHE_DIR" "$TF_CLI_CONFIG_FILE"; do check_path "$path"; done
config_temp="$(mktemp "$HOME/cli.XXXXXX")"
trap 'rm -f "$config_temp"' EXIT HUP INT TERM
printf 'plugin_cache_dir = "%s"\ndisable_checkpoint = true\nprovider_installation {\n  direct {}\n}\n' "$TF_PLUGIN_CACHE_DIR" > "$config_temp"
test ! -d "$TF_CLI_CONFIG_FILE" || { echo 'tofu_config_is_directory' >&2; exit 1; }
mv -f "$config_temp" "$TF_CLI_CONFIG_FILE"
"#
    );
    velnor_actions_workflow_renderer::shell_step(
        "Prepare isolated Tofu configuration",
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            script.to_owned(),
            "tofu-isolation".to_owned(),
            velnor_actions_tofu::key_for_root(root),
        ],
        task_env(root)?,
    )
    .map_err(OrchestratorError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use velnor_actions_contract::StepKind;

    #[test]
    fn config_paths_are_private_and_distinct_from_provider_payload() {
        let env = task_env("stacks/vpc").expect("safe root");
        assert!(env["TF_CLI_CONFIG_FILE"].ends_with("/cli.tfrc"));
        assert_ne!(env["HOME"], env["TF_PLUGIN_CACHE_DIR"]);
        assert_ne!(env["TF_DATA_DIR"], env["TF_PLUGIN_CACHE_DIR"]);
        assert!(!env.contains_key("TF_PLUGIN_CACHE_MAY_BREAK_DEPENDENCY_LOCK_FILE"));
        assert_ne!(
            env["HOME"],
            task_env("stacks/db").expect("safe root")["HOME"]
        );
    }

    #[test]
    fn config_materialization_uses_direct_sources_without_mirrors_or_overrides() {
        let step = materialization_step("").expect("build step");
        let StepKind::Shell { run, env } = step.kind else {
            panic!("shell step");
        };
        let script = &run[2];
        assert!(script.contains("test ! -L"));
        assert!(script.contains("plugin_cache_dir ="));
        assert!(script.contains("disable_checkpoint = true"));
        assert!(!script.contains("dev_overrides"));
        assert!(script.contains("provider_installation {\\n  direct {}\\n}"));
        assert!(!script.contains("filesystem_mirror"));
        assert!(!script.contains("network_mirror"));
        assert!(env["TF_CLI_CONFIG_FILE"].starts_with(HOME_BASE));
    }

    #[test]
    fn child_prefix_drops_every_unlisted_ambient_variable() {
        let prefix = isolation_prefix();
        assert!(prefix.starts_with("env -i "));
        assert!(prefix.contains("TF_CLI_CONFIG_FILE=\"$TF_CLI_CONFIG_FILE\""));
        for forbidden in [
            "AWS_",
            "TF_VAR_",
            "TF_TOKEN_",
            "GITHUB_TOKEN",
            "DEV_OVERRIDES",
        ] {
            assert!(!prefix.contains(forbidden), "{forbidden}");
        }
    }
}

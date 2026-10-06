//! Safe selected-tool bootstrap and current-source guard for native jobs.

use crate::RenderError;
use crate::build_task_jobs::{BuildTaskPolicy, CARGO_BINSTALL_ONLY_ENV};

const JQ_VERSION: &str = "jq-1.8.2";
const MISE_INSTALL_COMMAND: &str = "mise --no-env --locked --no-hooks install --jobs 2";
const MISE_CONFIG_LIST_COMMAND: &str = "mise --no-env --no-hooks config ls --json";
const MISE_RUN_PREFIX: &str = "mise --no-env --locked --no-hooks run --skip-tools";

pub(super) use crate::build_task_mise_tools::selected_mise_files;
pub(super) use crate::build_task_mise_tools::validate_policy;

pub(super) fn install_selected_tools_script(
    policy: &BuildTaskPolicy,
) -> Result<String, RenderError> {
    let (config, lock) = selected_mise_files(policy)?;
    let task_root = task_root_expr(&policy.task.id);
    let mut lines = base_environment(&task_root);
    lines.extend([
        "[[ \"$GITHUB_RUN_ID\" =~ ^[0-9]+$ ]]".to_owned(),
        "[[ \"$GITHUB_RUN_ATTEMPT\" =~ ^[0-9]+$ ]]".to_owned(),
        "/bin/mkdir -m 700 \"$task_root\"".to_owned(),
        concat!(
            "/bin/mkdir -m 700 \"$task_root/home\" \"$task_root/config\" ",
            "\"$task_root/data\" \"$task_root/cache\" \"$task_root/state\" ",
            "\"$task_root/cargo\" \"$task_root/rustup\"",
        )
        .to_owned(),
        private_environment(),
        "export MISE_CEILING_PATHS=\"$task_root\"".to_owned(),
        "export MISE_TRUSTED_CONFIG_PATHS=\"$task_root\"".to_owned(),
        "export MISE_NO_HOOKS=1".to_owned(),
        format!("export {CARGO_BINSTALL_ONLY_ENV}=1"),
        ": > \"$task_root/global.toml\"".to_owned(),
        ": > \"$task_root/system.toml\"".to_owned(),
        heredoc(
            "$task_root/mise.toml",
            "VELNOR_SELECTED_MISE_CONFIG",
            &config,
        ),
        heredoc("$task_root/mise.lock", "VELNOR_SELECTED_MISE_LOCK", &lock),
        "cd -P \"$task_root\"".to_owned(),
        jq_guard(),
        config_chain_check(&[
            "$task_root/mise.toml",
            "$task_root/global.toml",
            "$task_root/system.toml",
        ]),
        MISE_INSTALL_COMMAND.to_owned(),
    ]);
    Ok(lines.join("\n"))
}

pub(super) fn source_guard_script(policy: &BuildTaskPolicy) -> Result<String, RenderError> {
    let task_root = task_root_expr(&policy.task.id);
    let mbx = policy
        .selected_tools
        .iter()
        .find(|tool| tool.key == "mr-boxington")
        .ok_or_else(|| RenderError::InvalidWorkflow("build_task_mbx_missing".to_owned()))?;
    let mut lines = base_environment(&task_root);
    lines.extend([
        "test -d \"$task_root\"".to_owned(),
        private_environment(),
        "export MISE_TRUSTED_CONFIG_PATHS=\"$GITHUB_WORKSPACE\"".to_owned(),
        "export MISE_CEILING_PATHS=\"$GITHUB_WORKSPACE\"".to_owned(),
        "export MISE_NO_HOOKS=1".to_owned(),
        "export MBX_CARGO_SHIM_MODE=1".to_owned(),
        format!("export {CARGO_BINSTALL_ONLY_ENV}=1"),
        "unset MISE_CONFIG_FILE MISE_ENV MISE_ENV_FILE".to_owned(),
        "workspace_root=\"$(cd -P \"$GITHUB_WORKSPACE\" && pwd -P)\"".to_owned(),
        "cd -P \"$workspace_root\"".to_owned(),
        source_hash_check("mise.toml", &policy.mise_config_sha256),
        source_hash_check("mise.lock", &policy.mise_lock_sha256),
        source_hash_check("rust-toolchain.toml", &policy.rust_toolchain_sha256),
        jq_guard(),
        config_chain_check(&[
            "$workspace_root/mise.toml",
            "$workspace_root/rust-toolchain.toml",
            "$task_root/global.toml",
            "$task_root/system.toml",
        ]),
        "mise --no-env --no-hooks config get wrappers.cargo.command --file mise.toml | /usr/bin/grep -Fqx mbx".to_owned(),
        "mise --no-env --no-hooks config get wrappers.cargo.env.MBX_CARGO_SHIM_MODE --file mise.toml | /usr/bin/grep -Fqx 1".to_owned(),
        "mbx_path=\"$(mise --no-env --no-hooks which mbx)\"".to_owned(),
        format!(
            "case \"$mbx_path\" in \"$MISE_DATA_DIR/installs/mr-boxington/{}/\"*) ;; *) exit 1 ;; esac",
            mbx.version
        ),
        "test -x \"$mbx_path\" && test ! -L \"$mbx_path\"".to_owned(),
        "mbx_parent=\"$(cd -P \"$(/usr/bin/dirname \"$mbx_path\")\" && pwd -P)\"".to_owned(),
        "test \"$mbx_path\" = \"$mbx_parent/$(/usr/bin/basename \"$mbx_path\")\"".to_owned(),
        "mbx_command=\"$(mise --no-env --locked --no-hooks exec -- sh -c 'command -v mbx')\"".to_owned(),
        "test \"$mbx_command\" = \"$mbx_path\"".to_owned(),
        "cargo_path=\"$(mise --no-env --locked --no-hooks exec -- sh -c 'command -v cargo')\"".to_owned(),
        "test \"$cargo_path\" = \"$MISE_DATA_DIR/command-wrappers/bin/cargo\"".to_owned(),
        "test -x \"$cargo_path\" && test -L \"$cargo_path\"".to_owned(),
        "mise_path=\"$(mise --no-env --locked --no-hooks exec -- sh -c 'command -v mise')\"".to_owned(),
        "case \"$mise_path\" in /*) ;; *) exit 1 ;; esac".to_owned(),
        "test -x \"$mise_path\"".to_owned(),
        "mise_target=\"$(/usr/bin/readlink \"$cargo_path\")\"".to_owned(),
        "case \"$mise_target\" in /*) ;; *) exit 1 ;; esac".to_owned(),
        "test \"$mise_target\" = \"$mise_path\"".to_owned(),
        "mise --no-env --locked --no-hooks exec -- cargo --version".to_owned(),
    ]);
    Ok(lines.join("\n"))
}

pub(super) fn run_build_task_script(policy: &BuildTaskPolicy) -> Result<String, RenderError> {
    let mut script = source_guard_script(policy)?;
    script.push('\n');
    script.push_str(&format!(
        "{MISE_RUN_PREFIX} {}",
        policy.task.mise_task
    ));
    Ok(script)
}

fn base_environment(task_root: &str) -> Vec<String> {
    vec![
        "set -euo pipefail".to_owned(),
        "for name in ${!MISE_@}; do unset \"$name\"; done".to_owned(),
        "unset GITHUB_TOKEN GH_TOKEN ACTIONS_RUNTIME_TOKEN ACTIONS_ID_TOKEN_REQUEST_TOKEN"
            .to_owned(),
        "umask 077".to_owned(),
        format!("task_root=\"{task_root}\""),
        "test -n \"$RUNNER_TEMP\" && test -n \"$GITHUB_RUN_ID\" && test -n \"$GITHUB_RUN_ATTEMPT\""
            .to_owned(),
    ]
}

fn private_environment() -> String {
    concat!(
        "export HOME=\"$task_root/home\"\n",
        "export XDG_CONFIG_HOME=\"$task_root/home/config\"\n",
        "export XDG_DATA_HOME=\"$task_root/home/data\"\n",
        "export XDG_CACHE_HOME=\"$task_root/home/cache\"\n",
        "export XDG_STATE_HOME=\"$task_root/home/state\"\n",
        "export MISE_CONFIG_DIR=\"$task_root/config\"\n",
        "export MISE_DATA_DIR=\"$task_root/data\"\n",
        "export MISE_CACHE_DIR=\"$task_root/cache\"\n",
        "export MISE_STATE_DIR=\"$task_root/state\"\n",
        "export CARGO_HOME=\"$task_root/cargo\"\n",
        "export RUSTUP_HOME=\"$task_root/rustup\"\n",
        "export MISE_GLOBAL_CONFIG_FILE=\"$task_root/global.toml\"\n",
        "export MISE_SYSTEM_CONFIG_FILE=\"$task_root/system.toml\"\n",
        "export MISE_AUTO_INSTALL=false\n",
        "export MISE_EXEC_AUTO_INSTALL=false\n",
        "export MISE_TASK_RUN_AUTO_INSTALL=false\n",
        "export MISE_AUTO_ENV=false\n",
        "export MISE_NO_ENV=1\n",
    )
    .to_owned()
}

fn task_root_expr(task_id: &str) -> String {
    format!("${{RUNNER_TEMP}}/velnor-task-${{GITHUB_RUN_ID}}-${{GITHUB_RUN_ATTEMPT}}-{task_id}")
}

fn heredoc(path: &str, delimiter: &str, content: &str) -> String {
    format!("/usr/bin/cat > \"{path}\" <<'{delimiter}'\n{content}\n{delimiter}")
}

fn jq_guard() -> String {
    format!(
        "jq_path=\"$(command -v jq)\"\ntest -x \"$jq_path\"\n\"$jq_path\" --version | /usr/bin/grep -Fqx '{JQ_VERSION}'"
    )
}

fn config_chain_check(expected: &[&str]) -> String {
    let expected_lines = expected
        .iter()
        .map(|path| format!("\"{path}\""))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "expected_configs=\"$(printf '%s\\n' {expected_lines} | LC_ALL=C /usr/bin/sort)\"\nactual_configs=\"$({MISE_CONFIG_LIST_COMMAND} | \"$jq_path\" -r '.[].path' | LC_ALL=C /usr/bin/sort)\"\ntest \"$actual_configs\" = \"$expected_configs\""
    )
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::private_environment;

    #[test]
    fn generated_private_environment_assigns_exact_values() {
        const VARIABLES: [&str; 18] = [
            "HOME",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_CACHE_HOME",
            "XDG_STATE_HOME",
            "MISE_CONFIG_DIR",
            "MISE_DATA_DIR",
            "MISE_CACHE_DIR",
            "MISE_STATE_DIR",
            "CARGO_HOME",
            "RUSTUP_HOME",
            "MISE_GLOBAL_CONFIG_FILE",
            "MISE_SYSTEM_CONFIG_FILE",
            "MISE_AUTO_INSTALL",
            "MISE_EXEC_AUTO_INSTALL",
            "MISE_TASK_RUN_AUTO_INSTALL",
            "MISE_AUTO_ENV",
            "MISE_NO_ENV",
        ];
        const EXPECTED: [&str; 18] = [
            "/task-root/home",
            "/task-root/home/config",
            "/task-root/home/data",
            "/task-root/home/cache",
            "/task-root/home/state",
            "/task-root/config",
            "/task-root/data",
            "/task-root/cache",
            "/task-root/state",
            "/task-root/cargo",
            "/task-root/rustup",
            "/task-root/global.toml",
            "/task-root/system.toml",
            "false",
            "false",
            "false",
            "false",
            "1",
        ];
        let values = VARIABLES
            .iter()
            .map(|name| format!("\"${name}\""))
            .collect::<Vec<_>>()
            .join(" ");
        let script = format!(
            "set -euo pipefail\ntask_root=/task-root\n{}\nprintf '%s\\0' {values}\n",
            private_environment()
        );
        let output = Command::new("/bin/bash")
            .args(["--noprofile", "--norc", "-c"])
            .arg(script)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .output()
            .expect("run isolated environment fixture");
        assert!(
            output.status.success(),
            "shell fixture failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let actual = output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|value| !value.is_empty())
            .map(|value| String::from_utf8(value.to_vec()).expect("UTF-8 env value"))
            .collect::<Vec<_>>();
        assert_eq!(
            actual,
            EXPECTED.map(str::to_owned),
            "every isolated tool and Mise setting has its exact value"
        );
    }
}

fn source_hash_check(path: &str, sha256: &str) -> String {
    format!(
        "test -f \"$workspace_root/{path}\" && test ! -L \"$workspace_root/{path}\"\nactual=\"$(/usr/bin/shasum -a 256 \"$workspace_root/{path}\" | /usr/bin/awk '{{print $1}}')\"\ntest \"$actual\" = '{sha256}'"
    )
}

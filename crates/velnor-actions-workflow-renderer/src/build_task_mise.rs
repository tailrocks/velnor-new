//! Safe selected-tool bootstrap and current-source guard for native jobs.
//!
//! Every emitted guard is one single-line `bash -c` script: statements join
//! with `;`, captures use the `> file` plus `read` idiom, and file writes
//! use `printf`. Newlines, command substitution, and backticks never appear,
//! per the renderer's command policy.

use crate::RenderError;
use crate::verification_jobs::build_task_jobs::{BuildTaskPolicy, CARGO_BINSTALL_ONLY_ENV};
use crate::verification_jobs::task_script::{capture_command, jq_guard, printf_write};

const MISE_INSTALL_COMMAND: &str = "mise --no-env --locked --no-hooks install --jobs 2";
const MISE_CONFIG_LIST_COMMAND: &str = "mise --no-env --no-hooks config ls --json";
const MISE_RUN_PREFIX: &str = "mise --no-env --locked --no-hooks run --skip-tools";

pub(super) use crate::verification_jobs::build_task_mise_tools::selected_mise_files;
pub(super) use crate::verification_jobs::build_task_mise_tools::validate_policy;

pub(super) fn install_selected_tools_script(
    policy: &BuildTaskPolicy,
) -> Result<String, RenderError> {
    let (config, lock) = selected_mise_files(policy)?;
    let task_root = task_root_expr(&policy.task.id);
    let mut statements = base_environment(&task_root);
    statements.extend([
        "[[ \"$GITHUB_RUN_ID\" =~ ^[0-9]+$ ]]".to_owned(),
        "[[ \"$GITHUB_RUN_ATTEMPT\" =~ ^[0-9]+$ ]]".to_owned(),
        "/bin/mkdir -m 700 \"$task_root\"".to_owned(),
        concat!(
            "/bin/mkdir -m 700 \"$task_root/home\" \"$task_root/config\" ",
            "\"$task_root/data\" \"$task_root/cache\" \"$task_root/state\" ",
            "\"$task_root/cargo\" \"$task_root/rustup\"",
        )
        .to_owned(),
    ]);
    statements.extend(private_environment());
    statements.extend([
        // Mise excludes the ceiling directory itself, so use the private
        // root's parent to make the generated config the first loaded file.
        "export MISE_CEILING_PATHS=\"$task_root/..\"".to_owned(),
        "export MISE_TRUSTED_CONFIG_PATHS=\"$task_root\"".to_owned(),
        "export MISE_NO_HOOKS=1".to_owned(),
        format!("export {CARGO_BINSTALL_ONLY_ENV}=1"),
        ": > \"$task_root/global.toml\"".to_owned(),
        ": > \"$task_root/system.toml\"".to_owned(),
        printf_write("$task_root/mise.toml", &config, "build_task")?,
        printf_write("$task_root/mise.lock", &lock, "build_task")?,
        "cd -P \"$task_root\"".to_owned(),
        jq_guard(),
        config_chain_check([
            "$task_root/mise.toml",
            "$task_root/global.toml",
            "$task_root/system.toml",
        ]),
        MISE_INSTALL_COMMAND.to_owned(),
    ]);
    Ok(statements.join("; "))
}

pub(super) fn source_guard_script(policy: &BuildTaskPolicy) -> Result<String, RenderError> {
    let task_root = task_root_expr(&policy.task.id);
    let mbx = policy
        .selected_tools
        .iter()
        .find(|tool| tool.key == "mr-boxington")
        .ok_or_else(|| RenderError::InvalidWorkflow("build_task_mbx_missing".to_owned()))?;
    let mut statements = base_environment(&task_root);
    statements.extend(["test -d \"$task_root\"".to_owned()]);
    statements.extend(private_environment());
    statements.extend([
        "export MISE_TRUSTED_CONFIG_PATHS=\"$GITHUB_WORKSPACE\"".to_owned(),
        "export MISE_CEILING_PATHS=\"$GITHUB_WORKSPACE/..\"".to_owned(),
        "export MISE_NO_HOOKS=1".to_owned(),
        "export MBX_CARGO_SHIM_MODE=1".to_owned(),
        format!("export {CARGO_BINSTALL_ONLY_ENV}=1"),
        "unset MISE_CONFIG_FILE MISE_ENV MISE_ENV_FILE".to_owned(),
        "cd -P \"$GITHUB_WORKSPACE\"".to_owned(),
        "workspace_root=\"$PWD\"".to_owned(),
        "workspace_ceiling=\"$workspace_root/..\"".to_owned(),
        "cd -P \"$workspace_ceiling\"".to_owned(),
        "workspace_ceiling=\"$PWD\"".to_owned(),
        "cd -P \"$workspace_root\"".to_owned(),
        "export MISE_CEILING_PATHS=\"$workspace_ceiling\"".to_owned(),
        repository_path_guards("mise.toml", false),
        repository_path_guards("mise.lock", false),
        repository_path_guards("rust-toolchain.toml", false),
        source_hash_check("mise.toml", Some(&policy.mise_config_sha256)),
        source_hash_check("mise.lock", Some(&policy.mise_lock_sha256)),
        source_hash_check("rust-toolchain.toml", Some(&policy.rust_toolchain_sha256)),
        source_hash_check(
            &policy.task.source.mise_config,
            Some(&policy.source_mise_config_sha256),
        ),
    ]);
    let source_lock_path = policy.task.source.mise_lock_path();
    if source_lock_path != "mise.lock" {
        statements.push(source_hash_check(
            &source_lock_path,
            policy.source_mise_lock_sha256.as_deref(),
        ));
    }
    let source_rust_path = policy.task.source.rust_toolchain_path();
    if source_rust_path != "rust-toolchain.toml" {
        statements.push(source_hash_check(
            &source_rust_path,
            policy.source_rust_toolchain_sha256.as_deref(),
        ));
    }
    statements.extend([
        repository_path_guards(&policy.task.source.working_directory, true),
        format!(
            "cd -P \"$workspace_root/{}\"",
            policy.task.source.working_directory
        ),
        "task_working_directory=\"$PWD\"".to_owned(),
        jq_guard(),
        config_chain_check(expected_config_chain(policy).iter().map(String::as_str)),
        "mise --no-env --no-hooks config get wrappers.cargo.command --file \"$workspace_root/mise.toml\" | /usr/bin/grep -Fqx mbx".to_owned(),
        "mise --no-env --no-hooks config get wrappers.cargo.env.MBX_CARGO_SHIM_MODE --file \"$workspace_root/mise.toml\" | /usr/bin/grep -Fqx 1".to_owned(),
    ]);
    statements.extend(mbx_guard(&mbx.version));
    statements.push("mise --no-env --locked --no-hooks exec -- cargo --version".to_owned());
    Ok(statements.join("; "))
}

/// Pinned-MBX and Cargo-wrapper identity checks without command substitution.
fn mbx_guard(mbx_version: &str) -> Vec<String> {
    vec![
        capture_command(
            "mise --no-env --no-hooks which mbx",
            "$task_root/mbx_path.txt",
            "mbx_path",
        ),
        format!(
            "case \"$mbx_path\" in \"$MISE_DATA_DIR/installs/mr-boxington/{mbx_version}/\"*) ;; *) exit 1 ;; esac"
        ),
        "test -x \"$mbx_path\"".to_owned(),
        "test ! -L \"$mbx_path\"".to_owned(),
        "mbx_dir=\"${mbx_path%/*}\"".to_owned(),
        "cd -P \"$mbx_dir\"".to_owned(),
        "mbx_parent=\"$PWD\"".to_owned(),
        "cd -P \"$task_working_directory\"".to_owned(),
        "mbx_base=\"${mbx_path##*/}\"".to_owned(),
        "test \"$mbx_path\" = \"$mbx_parent/$mbx_base\"".to_owned(),
        capture_command(
            "mise --no-env --locked --no-hooks exec -- sh -c 'command -v mbx'",
            "$task_root/mbx_command.txt",
            "mbx_command",
        ),
        "test \"$mbx_command\" = \"$mbx_path\"".to_owned(),
        capture_command(
            "mise --no-env --locked --no-hooks exec -- sh -c 'command -v cargo'",
            "$task_root/cargo_path.txt",
            "cargo_path",
        ),
        "test \"$cargo_path\" = \"$MISE_DATA_DIR/command-wrappers/bin/cargo\"".to_owned(),
        "test -x \"$cargo_path\"".to_owned(),
        "test -L \"$cargo_path\"".to_owned(),
        capture_command(
            "mise --no-env --locked --no-hooks exec -- sh -c 'command -v mise'",
            "$task_root/mise_path.txt",
            "mise_path",
        ),
        "case \"$mise_path\" in /*) ;; *) exit 1 ;; esac".to_owned(),
        "test -x \"$mise_path\"".to_owned(),
        capture_command(
            "/usr/bin/readlink \"$cargo_path\"",
            "$task_root/mise_target.txt",
            "mise_target",
        ),
        "case \"$mise_target\" in /*) ;; *) exit 1 ;; esac".to_owned(),
        "test \"$mise_target\" = \"$mise_path\"".to_owned(),
    ]
}

pub(super) fn run_build_task_script(policy: &BuildTaskPolicy) -> Result<String, RenderError> {
    let guard = source_guard_script(policy)?;
    Ok(format!(
        "{guard}; {MISE_RUN_PREFIX} {}",
        policy.task.mise_task
    ))
}

fn base_environment(task_root: &str) -> Vec<String> {
    vec![
        "set -euo pipefail".to_owned(),
        // Credential unsets come from the renderer's injected prelude, which
        // must stay the only token-naming text for the token-hygiene gate.
        "for name in ${!MISE_@}; do unset \"$name\"; done".to_owned(),
        "umask 077".to_owned(),
        format!("task_root=\"{task_root}\""),
        "test -n \"$RUNNER_TEMP\" && test -n \"$GITHUB_RUN_ID\" && test -n \"$GITHUB_RUN_ATTEMPT\""
            .to_owned(),
    ]
}

fn private_environment() -> Vec<String> {
    [
        "HOME=\"$task_root/home\"",
        "XDG_CONFIG_HOME=\"$task_root/home/config\"",
        "XDG_DATA_HOME=\"$task_root/home/data\"",
        "XDG_CACHE_HOME=\"$task_root/home/cache\"",
        "XDG_STATE_HOME=\"$task_root/home/state\"",
        "MISE_CONFIG_DIR=\"$task_root/config\"",
        "MISE_DATA_DIR=\"$task_root/data\"",
        "MISE_CACHE_DIR=\"$task_root/cache\"",
        "MISE_STATE_DIR=\"$task_root/state\"",
        "CARGO_HOME=\"$task_root/cargo\"",
        "RUSTUP_HOME=\"$task_root/rustup\"",
        "MISE_GLOBAL_CONFIG_FILE=\"$task_root/global.toml\"",
        "MISE_SYSTEM_CONFIG_FILE=\"$task_root/system.toml\"",
        "MISE_AUTO_INSTALL=false",
        "MISE_EXEC_AUTO_INSTALL=false",
        "MISE_TASK_RUN_AUTO_INSTALL=false",
        "MISE_AUTO_ENV=false",
        "MISE_NO_ENV=1",
    ]
    .iter()
    .map(|assignment| format!("export {assignment}"))
    .collect()
}

fn task_root_expr(task_id: &str) -> String {
    format!("${{RUNNER_TEMP}}/velnor-task-${{GITHUB_RUN_ID}}-${{GITHUB_RUN_ATTEMPT}}-{task_id}")
}

fn config_chain_check<'a>(expected: impl IntoIterator<Item = &'a str>) -> String {
    let expected_args = expected
        .into_iter()
        .map(|path| format!("\"{path}\""))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "{MISE_CONFIG_LIST_COMMAND} | \"$jq_path\" -r '.[].path' | LC_ALL=C /usr/bin/sort > \"$task_root/actual_mise_configs.txt\"; printf '%s\\n' {expected_args} | LC_ALL=C /usr/bin/sort | /usr/bin/cmp -s \"$task_root/actual_mise_configs.txt\" -"
    )
}

fn source_hash_check(path: &str, sha256: Option<&str>) -> String {
    let guard = repository_path_guards(path, false);
    match sha256 {
        Some(sha256) => format!(
            "{guard}; test -f \"$workspace_root/{path}\"; test ! -L \"$workspace_root/{path}\"; /usr/bin/shasum -a 256 \"$workspace_root/{path}\" > \"$task_root/sha256.txt\"; read actual rest < \"$task_root/sha256.txt\"; test \"$actual\" = '{sha256}'"
        ),
        None => format!(
            "{guard}; test ! -e \"$workspace_root/{path}\"; test ! -L \"$workspace_root/{path}\""
        ),
    }
}

fn repository_path_guards(path: &str, directory: bool) -> String {
    let components = path.split('/').collect::<Vec<_>>();
    let prefix_len = if directory {
        components.len()
    } else {
        components.len().saturating_sub(1)
    };
    let guards = (1..=prefix_len)
        .map(|length| {
            let prefix = components[..length].join("/");
            format!("test -d \"$workspace_root/{prefix}\"; test ! -L \"$workspace_root/{prefix}\"")
        })
        .collect::<Vec<_>>()
        .join("; ");
    if guards.is_empty() {
        ":".to_owned()
    } else {
        guards
    }
}

fn expected_config_chain(policy: &BuildTaskPolicy) -> Vec<String> {
    let mut paths = vec!["$workspace_root/mise.toml".to_owned()];
    if policy.task.source.mise_config != "mise.toml" {
        paths.push(format!(
            "$workspace_root/{}",
            policy.task.source.mise_config
        ));
    }
    paths.push("$workspace_root/rust-toolchain.toml".to_owned());
    let source_rust = policy.task.source.rust_toolchain_path();
    if source_rust != "rust-toolchain.toml" && policy.source_rust_toolchain_sha256.is_some() {
        paths.push(format!("$workspace_root/{source_rust}"));
    }
    paths.extend([
        "$task_root/global.toml".to_owned(),
        "$task_root/system.toml".to_owned(),
    ]);
    paths
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
            "set -euo pipefail; task_root=/task-root; {}; printf '%s\\0' {values};\n",
            private_environment().join("; ")
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

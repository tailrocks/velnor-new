// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

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
        "export MISE_CEILING_PATHS=\"${task_root%/*}\"".to_owned(),
        "export MISE_TRUSTED_CONFIG_PATHS=\"$task_root\"".to_owned(),
        "export MISE_NO_HOOKS=1".to_owned(),
        format!("export {CARGO_BINSTALL_ONLY_ENV}=1"),
        ": > \"$task_root/global.toml\"".to_owned(),
        ": > \"$task_root/system.toml\"".to_owned(),
        printf_write("$task_root/mise.toml", &config, "build_task")?,
        printf_write("$task_root/mise.lock", &lock, "build_task")?,
        "cd -P \"$task_root\"".to_owned(),
        jq_guard(),
        config_chain_check(&[
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
    let rust = policy
        .selected_tools
        .iter()
        .find(|tool| tool.key == "rust")
        .ok_or_else(|| RenderError::InvalidWorkflow("build_task_rust_missing".to_owned()))?;
    let mut statements = base_environment(&task_root);
    statements.extend(["test -d \"$task_root\"".to_owned()]);
    statements.extend(private_environment());
    statements.extend([
        "export MISE_TRUSTED_CONFIG_PATHS=\"$GITHUB_WORKSPACE\"".to_owned(),
        "export MISE_CEILING_PATHS=\"$GITHUB_WORKSPACE/..\"".to_owned(),
        "export MISE_NO_HOOKS=1".to_owned(),
        format!("export {CARGO_BINSTALL_ONLY_ENV}=1"),
        "unset MISE_CONFIG_FILE MISE_ENV MISE_ENV_FILE".to_owned(),
        "cd -P \"$GITHUB_WORKSPACE\"".to_owned(),
        "workspace_root=\"$PWD\"".to_owned(),
        "cd -P \"$workspace_root\"".to_owned(),
        source_hash_check("mise.toml", &policy.mise_config_sha256),
        source_hash_check("mise.lock", &policy.mise_lock_sha256),
        source_hash_check("rust-toolchain.toml", &policy.rust_toolchain_sha256),
    ]);
    if policy.task.source.mise_config != "mise.toml" {
        statements.push(source_hash_check(
            &policy.task.source.mise_config,
            &policy.source_mise_config_sha256,
        ));
    }
    if let Some(sha) = &policy.source_mise_lock_sha256 {
        statements.push(source_hash_check(&policy.task.source.mise_lock_path(), sha));
    }
    if let Some(sha) = &policy.source_rust_toolchain_sha256 {
        statements.push(source_hash_check(
            &policy.task.source.rust_toolchain_path(),
            sha,
        ));
    }
    let mut expected_configs = vec![
        "$workspace_root/mise.toml".to_owned(),
        "$workspace_root/rust-toolchain.toml".to_owned(),
    ];
    if policy.task.source.mise_config != "mise.toml" {
        expected_configs.push(format!(
            "$workspace_root/{}",
            policy.task.source.mise_config
        ));
    }
    expected_configs.push("$task_root/global.toml".to_owned());
    expected_configs.push("$task_root/system.toml".to_owned());

    statements.push(jq_guard());
    statements.push(config_chain_check(&expected_configs));

    if policy.task.source.working_directory == "." {
        statements.push("task_working_directory=\"$workspace_root\"".to_owned());
    } else {
        statements.extend([
            "workspace_ceiling=\"$workspace_root/..\"".to_owned(),
            "export MISE_CEILING_PATHS=\"$workspace_ceiling\"".to_owned(),
            format!(
                "test ! -L \"$workspace_root/{}\"",
                policy.task.source.working_directory
            ),
            format!(
                "cd -P \"$workspace_root/{}\"",
                policy.task.source.working_directory
            ),
            "task_working_directory=\"$PWD\"".to_owned(),
        ]);
    }

    statements.extend(mbx_guard(&mbx.version, &rust.version));
    Ok(statements.join("; "))
}

fn mbx_guard(mbx_version: &str, rust_version: &str) -> Vec<String> {
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
        "cd -P \"$workspace_root\"".to_owned(),
        "mbx_base=\"${mbx_path##*/}\"".to_owned(),
        "test \"$mbx_path\" = \"$mbx_parent/$mbx_base\"".to_owned(),
        capture_command(
            "mise --no-env --locked --no-hooks which rustc",
            "$task_root/rustc_path.txt",
            "rustc_path",
        ),
        format!(
            "case \"$rustc_path\" in \"$MISE_DATA_DIR/installs/rust/{rust_version}/\"*) ;; *) exit 1 ;; esac"
        ),
        "test -x \"$rustc_path\"".to_owned(),
        "test ! -L \"$rustc_path\"".to_owned(),
        capture_command(
            "\"$mbx_path\" exec --project-root \"$workspace_root\" \"$rustc_path\" --version",
            "$task_root/rustc_version.txt",
            "rustc_version",
        ),
        format!("case \"$rustc_version\" in 'rustc {rust_version} ('*) ;; *) exit 1 ;; esac"),
        "cd -P \"$task_working_directory\"".to_owned(),
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

fn config_chain_check(expected: &[impl AsRef<str>]) -> String {
    let expected_args = expected
        .iter()
        .map(|path| format!("\"{}\"", path.as_ref()))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "{MISE_CONFIG_LIST_COMMAND} | \"$jq_path\" -r '.[].path' | LC_ALL=C /usr/bin/sort > \"$task_root/actual_mise_configs.txt\"; printf '%s\\n' {expected_args} | LC_ALL=C /usr/bin/sort | /usr/bin/cmp -s \"$task_root/actual_mise_configs.txt\" -"
    )
}

fn source_hash_check(path: &str, sha256: &str) -> String {
    format!(
        "test -f \"$workspace_root/{path}\"; test ! -L \"$workspace_root/{path}\"; /usr/bin/shasum -a 256 \"$workspace_root/{path}\" > \"$task_root/sha256.txt\"; read actual rest < \"$task_root/sha256.txt\"; test \"$actual\" = '{sha256}'"
    )
}

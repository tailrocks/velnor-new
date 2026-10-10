//! Selected, locked, prebuilt-only tool bootstrap for Verification tasks.
//!
//! Every emitted guard is one single-line `bash -c` script: statements join
//! with `;`, captures use the `> file` plus `read` idiom, and file writes
//! use `printf`. Newlines, command substitution, and backticks never appear,
//! per the renderer's command policy.

use std::collections::BTreeMap;

use crate::steps;
use crate::verification_jobs::build_task_jobs::BuildTaskTool;
use crate::verification_jobs::build_task_mise_tools::{
    BOLTFFI_KEY, BOLTFFI_MATCHING_REGEX, exact_rust_version, safe_backend, safe_identity,
    safe_version, selected_mise_files_for, validate_artifact,
};
use crate::verification_jobs::task_script::{capture_command, jq_guard, printf_write};
use crate::verification_jobs::{
    INSTALL_VERIFICATION_TOOLS_NAME, RUN_VERIFICATION_TASK_NAME, VerificationTaskPolicy,
};
use crate::{RenderError, mise_setup_step, shell_step};

const MBX_BACKEND: &str = "packslip:github.com/jdx/mr-boxington";
const MBX_TOOL_KEY: &str = "mr-boxington";

pub(crate) fn steps(
    policy: &VerificationTaskPolicy,
    checkout_uses: &str,
) -> Result<Vec<velnor_actions_contract::Step>, RenderError> {
    let env = BTreeMap::from([("MISE_CARGO_BINSTALL_ONLY".to_owned(), "1".to_owned())]);
    let mut rendered = vec![
        steps::checkout_step(checkout_uses)?,
        mise_setup_step(&policy.mise_setup)?,
    ];
    if !policy.selected_tools.is_empty() {
        rendered.push(shell_step(
            INSTALL_VERIFICATION_TOOLS_NAME,
            bash_script(&install_script(policy)?),
            env.clone(),
        )?);
    }
    rendered.push(shell_step(
        RUN_VERIFICATION_TASK_NAME,
        bash_script(&run_script(policy)),
        env,
    )?);
    Ok(rendered)
}

pub(crate) fn validate_policy(policy: &VerificationTaskPolicy) -> Result<(), RenderError> {
    for digest in [
        &policy.mise_config_sha256,
        &policy.mise_lock_sha256,
        &policy.rust_toolchain_sha256,
    ]
    .into_iter()
    .flatten()
    {
        if !velnor_actions_contract::ids::is_lower_hex_len(digest, 64) {
            return Err(RenderError::InvalidWorkflow(
                "verification_tool_source_sha256".to_owned(),
            ));
        }
    }
    if policy.mise_config_sha256.is_none() {
        return Err(RenderError::InvalidWorkflow(
            "verification_mise_config_missing".to_owned(),
        ));
    }
    if !policy.selected_tools.is_empty() && policy.mise_lock_sha256.is_none() {
        return Err(RenderError::InvalidWorkflow(
            "verification_tool_source_missing".to_owned(),
        ));
    }
    if policy.selected_tools.iter().any(|tool| tool.key == "rust")
        && policy.rust_toolchain_sha256.is_none()
    {
        return Err(RenderError::InvalidWorkflow(
            "verification_rust_toolchain_missing".to_owned(),
        ));
    }
    let (mise_os, _) = platform(policy.task.runner);
    let mut previous: Option<&str> = None;
    for tool in &policy.selected_tools {
        if !safe_identity(&tool.key)
            || !safe_version(&tool.version)
            || !safe_backend(&tool.backend)
            || previous.is_some_and(|key| key >= tool.key.as_str())
            || !(tool.os.is_empty() || tool.os == [mise_os])
        {
            return Err(RenderError::InvalidWorkflow(format!(
                "verification_tool_identity:{}",
                tool.key
            )));
        }
        previous = Some(&tool.key);
        validate_tool(tool)?;
    }
    Ok(())
}

fn validate_tool(tool: &BuildTaskTool) -> Result<(), RenderError> {
    if tool.key.starts_with("cargo:")
        || tool.backend.starts_with("cargo:")
        || matches!(
            tool.version.as_str(),
            "latest" | "system" | "ref" | "stable" | "nightly" | "lts"
        )
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "verification_cargo_source_install:{}",
            tool.key
        )));
    }
    if tool.key == "rust" {
        if tool.backend != "core:rust"
            || tool.artifact.is_some()
            || tool.config_options != tool.lock_options
            || !exact_rust_version(&tool.version)
            || tool
                .lock_options
                .keys()
                .any(|key| !matches!(key.as_str(), "components" | "targets"))
            || tool.lock_options.values().any(|value| !safe_option(value))
        {
            return Err(RenderError::InvalidWorkflow(
                "verification_rust_tool_pin".to_owned(),
            ));
        }
        return Ok(());
    }
    let artifact = tool.artifact.as_ref().ok_or_else(|| {
        RenderError::InvalidWorkflow(format!("verification_tool_artifact_missing:{}", tool.key))
    })?;
    let allowed_backend = (tool.key == MBX_TOOL_KEY && tool.backend == MBX_BACKEND)
        || (tool.key == BOLTFFI_KEY && tool.backend == BOLTFFI_KEY)
        || (tool.backend.starts_with("aqua:")
            && tool.backend.len() > "aqua:".len()
            && tool.config_options.is_empty())
        || (tool.key.starts_with("github:")
            && tool.key == tool.backend
            && tool.config_options.is_empty());
    let boltffi_options = BTreeMap::from([(
        "matching_regex".to_owned(),
        BOLTFFI_MATCHING_REGEX.to_owned(),
    )]);
    if !allowed_backend
        || tool.config_options != tool.lock_options
        || (tool.key == BOLTFFI_KEY
            && (tool.config_options != boltffi_options || tool.lock_options != boltffi_options))
        || (tool.key != BOLTFFI_KEY
            && (!tool.config_options.is_empty() || !tool.lock_options.is_empty()))
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "verification_tool_backend:{}:{}",
            tool.key, tool.backend
        )));
    }
    validate_artifact(&tool.key, artifact)
}

fn install_script(policy: &VerificationTaskPolicy) -> Result<String, RenderError> {
    let (mise_os, lock_platform) = platform(policy.task.runner);
    let (config, lock) = selected_mise_files_for(
        &policy.selected_tools,
        mise_os,
        lock_platform,
        policy
            .selected_tools
            .iter()
            .any(|tool| tool.key == MBX_TOOL_KEY && tool.backend == MBX_BACKEND),
    )?;
    let root = task_root(policy);
    let mut statements = base_environment(&root);
    statements.extend([
        "[[ \"$GITHUB_RUN_ID\" =~ ^[0-9]+$ ]]".to_owned(),
        "[[ \"$GITHUB_RUN_ATTEMPT\" =~ ^[0-9]+$ ]]".to_owned(),
        "/bin/mkdir -m 700 \"$task_root\"".to_owned(),
        "/bin/mkdir -m 700 \"$task_root/home\" \"$task_root/home/config\" \"$task_root/home/data\" \"$task_root/home/cache\" \"$task_root/home/state\" \"$task_root/config\" \"$task_root/data\" \"$task_root/cache\" \"$task_root/state\" \"$task_root/cargo\" \"$task_root/rustup\"".to_owned(),
    ]);
    statements.extend(private_environment());
    statements.extend([
        "export MISE_CEILING_PATHS=\"$task_root\"".to_owned(),
        "export MISE_TRUSTED_CONFIG_PATHS=\"$task_root\"".to_owned(),
        "export MISE_NO_HOOKS=1".to_owned(),
        "export MISE_CARGO_BINSTALL_ONLY=1".to_owned(),
        ": > \"$task_root/global.toml\"".to_owned(),
        ": > \"$task_root/system.toml\"".to_owned(),
        printf_write("$task_root/mise.toml", &config, "verification")?,
        printf_write("$task_root/mise.lock", &lock, "verification")?,
        "cd -P \"$task_root\"".to_owned(),
        jq_guard(),
        private_config_chain_check("$task_root/mise.toml"),
        "mise --no-env --locked --no-hooks install --jobs 2".to_owned(),
    ]);
    Ok(statements.join("; "))
}

fn run_script(policy: &VerificationTaskPolicy) -> String {
    let root = task_root(policy);
    let mut statements = base_environment(&root);
    statements.extend([
        "[[ \"$GITHUB_RUN_ID\" =~ ^[0-9]+$ ]]".to_owned(),
        "[[ \"$GITHUB_RUN_ATTEMPT\" =~ ^[0-9]+$ ]]".to_owned(),
        "cd -P \"$GITHUB_WORKSPACE\"".to_owned(),
        "workspace_root=\"$PWD\"".to_owned(),
    ]);
    statements.extend(repository_path_guards(
        &policy.task.source.working_directory,
        true,
    ));
    statements.push(format!(
        "cd -P \"$workspace_root/{}\"",
        policy.task.source.working_directory
    ));
    statements.push("task_working_directory=\"$PWD\"".to_owned());
    if policy.selected_tools.is_empty() {
        statements.extend([
            "/bin/mkdir -m 700 \"$task_root\"".to_owned(),
            "/bin/mkdir -m 700 \"$task_root/home\" \"$task_root/home/config\" \"$task_root/home/data\" \"$task_root/home/cache\" \"$task_root/home/state\" \"$task_root/config\" \"$task_root/data\" \"$task_root/cache\" \"$task_root/state\" \"$task_root/cargo\" \"$task_root/rustup\"".to_owned(),
            ": > \"$task_root/global.toml\"".to_owned(),
            ": > \"$task_root/system.toml\"".to_owned(),
        ]);
    } else {
        statements.push("test -d \"$task_root\"".to_owned());
    }
    statements.extend(private_environment());
    statements.extend([
        format!(
            "export MISE_CEILING_PATHS=\"$workspace_root/{}\"",
            policy.task.source.config_ceiling_directory()
        ),
        "export MISE_TRUSTED_CONFIG_PATHS=\"$workspace_root\"".to_owned(),
        "export MISE_NO_HOOKS=1".to_owned(),
        "export MISE_CARGO_BINSTALL_ONLY=1".to_owned(),
        "unset MISE_CONFIG_FILE MISE_ENV MISE_ENV_FILE".to_owned(),
        source_hash_check(
            &policy.task.source.mise_config,
            policy.mise_config_sha256.as_deref(),
            policy,
        ),
        source_hash_check(
            &policy.task.source.mise_lock_path(),
            policy.mise_lock_sha256.as_deref(),
            policy,
        ),
        source_hash_check(
            &policy.task.source.rust_toolchain_path(),
            policy.rust_toolchain_sha256.as_deref(),
            policy,
        ),
        jq_guard(),
        workspace_config_chain_check(policy),
    ]);
    if let Some(mbx) = policy
        .selected_tools
        .iter()
        .find(|tool| tool.key == MBX_TOOL_KEY && tool.backend == MBX_BACKEND)
    {
        statements.extend(mbx_guard(&mbx.version));
    }
    let locked = if policy.mise_lock_sha256.is_some() {
        "--locked "
    } else {
        ""
    };
    statements.push(format!(
        "mise --no-env {locked}--no-hooks run --skip-tools {}",
        policy.task.mise_task
    ));
    statements.join("; ")
}

/// Pinned-MBX identity checks without command substitution.
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
        "cd -P \"$workspace_root\"".to_owned(),
        "mbx_base=\"${mbx_path##*/}\"".to_owned(),
        "test \"$mbx_path\" = \"$mbx_parent/$mbx_base\"".to_owned(),
        "cd -P \"$task_working_directory\"".to_owned(),
    ]
}

fn base_environment(root: &str) -> Vec<String> {
    vec![
        "set -euo pipefail".to_owned(),
        // Credential unsets come from the renderer's injected prelude, which
        // must stay the only token-naming text for the token-hygiene gate.
        "for name in ${!MISE_@}; do unset \"$name\"; done".to_owned(),
        "umask 077".to_owned(),
        format!("task_root=\"{root}\""),
        "test -n \"$RUNNER_TEMP\" && test -n \"$GITHUB_WORKSPACE\" && test -n \"$GITHUB_RUN_ID\" && test -n \"$GITHUB_RUN_ATTEMPT\"".to_owned(),
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

fn source_hash_check(
    path: &str,
    expected: Option<&str>,
    policy: &VerificationTaskPolicy,
) -> String {
    let (hash_bin, hash_args) = match policy.task.runner {
        velnor_actions_contract::VerificationRunner::LinuxX64 => ("/usr/bin/sha256sum", ""),
        velnor_actions_contract::VerificationRunner::MacosArm64
        | velnor_actions_contract::VerificationRunner::Macos26Arm64 => {
            ("/usr/bin/shasum", "-a 256")
        }
    };
    let mut statements = repository_path_guards(path, false);
    statements.push(match expected {
        Some(expected) => format!(
            "test -f \"$workspace_root/{path}\"; test ! -L \"$workspace_root/{path}\"; {hash_bin} {hash_args} \"$workspace_root/{path}\" > \"$task_root/sha256.txt\"; read actual rest < \"$task_root/sha256.txt\"; test \"$actual\" = \"{expected}\""
        ),
        None => format!(
            "test ! -e \"$workspace_root/{path}\" && test ! -L \"$workspace_root/{path}\""
        ),
    });
    statements.join("; ")
}

fn repository_path_guards(path: &str, directory: bool) -> Vec<String> {
    let components = path.split('/').collect::<Vec<_>>();
    let prefix_len = if directory {
        components.len()
    } else {
        components.len().saturating_sub(1)
    };
    (1..=prefix_len)
        .map(|length| {
            let prefix = components[..length].join("/");
            format!("test -d \"$workspace_root/{prefix}\"; test ! -L \"$workspace_root/{prefix}\"")
        })
        .collect()
}

fn private_config_chain_check(expected: &str) -> String {
    format!(
        "mise --no-env --no-hooks config ls --json | \"$jq_path\" -r '.[].path' | LC_ALL=C /usr/bin/sort > \"$task_root/actual_mise_configs.txt\"; printf '%s\\n' \"{expected}\" \"$task_root/global.toml\" \"$task_root/system.toml\" | LC_ALL=C /usr/bin/sort | /usr/bin/cmp -s \"$task_root/actual_mise_configs.txt\" -"
    )
}

fn workspace_config_chain_check(policy: &VerificationTaskPolicy) -> String {
    let config_path = format!("$workspace_root/{}", policy.task.source.mise_config);
    let rust_path = format!(
        "$workspace_root/{}",
        policy.task.source.rust_toolchain_path()
    );
    let mut checks = vec![
        "mise --no-env --no-hooks config ls --json | \"$jq_path\" -r '.[].path' | LC_ALL=C /usr/bin/sort > \"$task_root/mise_configs.txt\"".to_owned(),
        "workspace_config_found=false".to_owned(),
        format!("while IFS= read -r path; do case \"$path\" in \"{config_path}\") workspace_config_found=true ;; \"{rust_path}\"|\"$task_root/global.toml\"|\"$task_root/system.toml\") ;; *) exit 1 ;; esac; done < \"$task_root/mise_configs.txt\""),
    ];
    checks.push(if policy.mise_config_sha256.is_some() {
        "[[ \"$workspace_config_found\" == true ]]".to_owned()
    } else {
        "[[ \"$workspace_config_found\" == false ]]".to_owned()
    });
    checks.join("; ")
}

fn task_root(policy: &VerificationTaskPolicy) -> String {
    format!(
        "${{RUNNER_TEMP}}/velnor-verification-${{GITHUB_RUN_ID}}-${{GITHUB_RUN_ATTEMPT}}-{}",
        policy.task.id
    )
}

/// Inline-shell argv so the script is single-quoted whole at render time.
///
/// Inner-shell variables must survive the outer shell; the script itself
/// starts with `set -euo pipefail`.
fn bash_script(script: &str) -> Vec<String> {
    vec!["bash".to_owned(), "-c".to_owned(), script.to_owned()]
}

fn platform(runner: velnor_actions_contract::VerificationRunner) -> (&'static str, &'static str) {
    match runner {
        velnor_actions_contract::VerificationRunner::LinuxX64 => ("linux", "linux-x64"),
        velnor_actions_contract::VerificationRunner::MacosArm64
        | velnor_actions_contract::VerificationRunner::Macos26Arm64 => ("macos", "macos-arm64"),
    }
}

fn safe_option(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b',' | b'.' | b'_' | b'-'))
}

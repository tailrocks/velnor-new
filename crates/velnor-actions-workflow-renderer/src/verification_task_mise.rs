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
    let (config, lock) = selected_mise_files_for(&policy.selected_tools, mise_os, lock_platform)?;
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

#[path = "verification_task_mise_environment.rs"]
mod environment;
use self::environment::{
    base_environment, bash_script, platform, private_config_chain_check, private_environment,
    repository_path_guards, safe_option, source_hash_check, task_root,
    workspace_config_chain_check,
};

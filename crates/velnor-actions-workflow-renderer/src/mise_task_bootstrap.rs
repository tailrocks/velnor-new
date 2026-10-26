// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::steps;
use crate::verification_jobs::build_task_jobs::BuildTaskTool;
use crate::verification_jobs::build_task_mise_tools::{
    BOLTFFI_KEY, BOLTFFI_MATCHING_REGEX, exact_rust_version, safe_backend, safe_identity,
    safe_version, selected_mise_files_for, valid_rust_toolchain_options, validate_artifact,
};
use crate::verification_jobs::task_script::{capture_command, jq_guard, printf_write};
use crate::verification_jobs::{INSTALL_VERIFICATION_TOOLS_NAME, MiseTaskBootstrapPolicy};
use crate::{RenderError, mise_setup_step, shell_step};
use velnor_actions_contract::{Step, StepKind, StepRole};

const MBX_BACKEND: &str = "packslip:github.com/jdx/mr-boxington";
const MBX_TOOL_KEY: &str = "mr-boxington";
pub(crate) const RUN_MISE_TASK_SUITE_NAME: &str = "Run declared Mise task suite";

pub(crate) fn steps(
    policy: &MiseTaskBootstrapPolicy,
    mise_setup: &crate::MiseSetup,
    checkout_uses: &str,
    full_event_sha: bool,
    compile_bounds: bool,
) -> Result<Vec<Step>, RenderError> {
    let mut env = BTreeMap::from([("MISE_CARGO_BINSTALL_ONLY".to_owned(), "1".to_owned())]);
    let mut rendered = vec![
        checkout_step(checkout_uses, full_event_sha)?,
        mise_setup_step(mise_setup)?,
    ];
    if !policy.selected_tools.is_empty() {
        rendered.push(shell_step(
            INSTALL_VERIFICATION_TOOLS_NAME,
            bash_script(&install_script(policy)?),
            env.clone(),
        )?);
    }
    if compile_bounds {
        env.insert("CARGO_BUILD_JOBS".to_owned(), "2".to_owned());
        env.insert("NEXTEST_TEST_THREADS".to_owned(), "2".to_owned());
    }
    rendered.push(shell_step(
        policy.run_step_name,
        bash_script(&run_script(policy)),
        env,
    )?);
    Ok(rendered)
}

fn checkout_step(uses: &str, full_event_sha: bool) -> Result<Step, RenderError> {
    if !full_event_sha {
        return steps::checkout_step(uses);
    }
    steps::validate_uses(uses)?;
    if !uses.starts_with("actions/checkout@") {
        return Err(RenderError::BadActionRef(format!("not_checkout:{uses}")));
    }
    steps::scan_for_private_subcommands(uses)?;
    Ok(Step {
        name: "Checkout event commit".to_owned(),
        id: None,
        parallel: false,
        role: Some(StepRole::Checkout),
        condition: None,
        kind: StepKind::Action {
            uses: uses.to_owned(),
            with: BTreeMap::from([
                ("fetch-depth".to_owned(), "0".to_owned()),
                ("persist-credentials".to_owned(), "false".to_owned()),
                ("ref".to_owned(), "${{ github.sha }}".to_owned()),
            ]),
            env: BTreeMap::new(),
        },
    })
}

pub(crate) fn validate_policy(policy: &MiseTaskBootstrapPolicy) -> Result<(), RenderError> {
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
    let (mise_os, _) = platform(policy.runner);
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
            || !valid_rust_toolchain_options(&tool.lock_options)
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

fn install_script(policy: &MiseTaskBootstrapPolicy) -> Result<String, RenderError> {
    let (mise_os, lock_platform) = platform(policy.runner);
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
        "export MISE_CEILING_PATHS=\"${task_root%/*}\"".to_owned(),
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

fn run_script(policy: &MiseTaskBootstrapPolicy) -> String {
    let root = task_root(policy);
    let mut statements = base_environment(&root);
    statements.extend([
        "[[ \"$GITHUB_RUN_ID\" =~ ^[0-9]+$ ]]".to_owned(),
        "[[ \"$GITHUB_RUN_ATTEMPT\" =~ ^[0-9]+$ ]]".to_owned(),
        "cd -P \"$GITHUB_WORKSPACE\"".to_owned(),
        "workspace_root=\"$PWD\"".to_owned(),
    ]);
    statements.extend(repository_path_guards(
        &policy.source.working_directory,
        true,
    ));
    statements.push(format!(
        "cd -P \"$workspace_root/{}\"",
        policy.source.working_directory
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
            policy.source.config_ceiling_directory()
        ),
        "export MISE_TRUSTED_CONFIG_PATHS=\"$workspace_root\"".to_owned(),
        "export MISE_NO_HOOKS=1".to_owned(),
        "export MISE_CARGO_BINSTALL_ONLY=1".to_owned(),
        "unset MISE_CONFIG_FILE MISE_ENV MISE_ENV_FILE".to_owned(),
        source_hash_check(
            &policy.source.mise_config,
            policy.mise_config_sha256.as_deref(),
            policy,
        ),
        source_hash_check(
            &policy.source.mise_lock_path(),
            policy.mise_lock_sha256.as_deref(),
            policy,
        ),
        source_hash_check(
            &policy.source.rust_toolchain_path(),
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
    if policy.evidence_input {
        statements.extend([
            "evidence_root=\"$workspace_root/.velnor/evidence\"".to_owned(),
            "test -d \"$evidence_root\" && test ! -L \"$evidence_root\"".to_owned(),
            "export VELNOR_GITHUB_EVIDENCE_ROOT=\"$evidence_root\"".to_owned(),
        ]);
    }
    let locked = if policy.mise_lock_sha256.is_some() {
        "--locked "
    } else {
        ""
    };
    statements.push(format!(
        "mise --no-env {locked}--no-hooks run --skip-tools {}",
        policy.mise_task
    ));
    statements.join("; ")
}

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

#[path = "mise_task_bootstrap_environment.rs"]
mod environment;
use self::environment::{
    base_environment, bash_script, platform, private_config_chain_check, private_environment,
    repository_path_guards, source_hash_check, task_root, workspace_config_chain_check,
};

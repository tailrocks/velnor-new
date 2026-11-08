// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeMap, ffi::OsString, fs, os::unix::fs::DirBuilderExt, path::Path};

use velnor_actions_contract::{NativeExecutionPurpose, native_execution_environment_script};
use velnor_actions_mise::{CheckDeadline, IsolatedCommand, PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::verifier_builders::{ToolchainPhase, verify_rust_owned_argv};

use super::{RunNativeQualificationRequest, source, validate};
use crate::{OrchestratorError, internal::internal};

const MAX_NATIVE_TOOL_BYTES: u64 = 192 * 1024 * 1024;

pub(super) fn run(request: &RunNativeQualificationRequest) -> Result<(), OrchestratorError> {
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return Err(internal("native_qualification_platform"));
    }
    let root = source::checked_root(&request.root)?;
    let runner_temp = &request.runner_temp;
    if !runner_temp.is_absolute() || runner_temp.starts_with(&root) {
        return Err(internal("native_qualification_temp"));
    }
    let source_commit = source::env("GITHUB_SHA")?;
    let head = source::git_text(
        &root,
        &velnor_actions_mise::GitRequest::rev_parse(vec!["HEAD".into()]),
        request.deadline,
    )?;
    if head.trim() != source_commit {
        return Err(internal("native_qualification_source_identity"));
    }
    validate::qualification_identity(&source_commit)?;
    source::ensure_clean_checkout(&root, request.deadline)?;
    super::qualification_service::verify(&root, runner_temp, request.deadline)?;
    let config = crate::config::load_config(&root)?;
    let catalog = crate::tool_catalog_resolve::resolve_tool_catalog(&root, &config)?;
    if catalog.version(PinnedTool::MrBoxington) != velnor_actions_contract::NATIVE_MBX_VERSION {
        return Err(internal("native_qualification_mbx_version"));
    }
    bootstrap(&root, runner_temp, request.deadline)?;
    prepare_rust(runner_temp, &catalog, request.deadline)?;
    execute(&root, runner_temp, &catalog, request.deadline)?;
    super::qualification_service::verify(&root, runner_temp, request.deadline)?;
    Ok(())
}

fn bootstrap(root: &Path, temp: &Path, deadline: CheckDeadline) -> Result<(), OrchestratorError> {
    let descriptor =
        velnor_actions_mise::catalog::source_build_bootstrap::native_linux_assets_json()
            .map_err(|_| internal("native_qualification_bootstrap_descriptor"))?;
    let env = BTreeMap::from([
        ("RUNNER_TEMP".to_owned(), path(temp)?),
        ("OWNED_TOOL_BUILD_BOOTSTRAP_JSON".to_owned(), descriptor),
    ]);
    let script = root.join("scripts/bootstrap-owned-tool-builder.py");
    run_env(
        root,
        &env,
        "/usr/bin/python3",
        &[
            "-I".into(),
            "-S".into(),
            path(&script)?,
            "--native-qualification".into(),
        ],
        deadline,
    )
}

fn prepare_rust(
    temp: &Path,
    catalog: &ToolCatalog,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let native = temp.join("velnor-qualification-runtime");
    verify_tool_bytes(&native)?;
    let home = native.join("bootstrap-home");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&home)
        .map_err(|_| internal("native_qualification_private_directory"))?;
    let env = tool_env(&native, &home, temp, catalog, false)?;
    let mise = path(&native.join("native-tools/mise"))?;
    run_env(
        temp,
        &env,
        &mise,
        &[
            "--no-config".into(),
            "--no-env".into(),
            "--no-hooks".into(),
            "install".into(),
            catalog.install_spec(PinnedTool::Rust),
        ],
        deadline,
    )?;
    let env = tool_env(&native, &home, temp, catalog, true)?;
    run_env(
        temp,
        &env,
        &mise,
        &[
            "--no-config".into(),
            "--no-env".into(),
            "--no-hooks".into(),
            "exec".into(),
            catalog.tool_spec(PinnedTool::Rust),
            "--".into(),
            "rustup".into(),
            "component".into(),
            "add".into(),
            "--toolchain".into(),
            catalog.rust_toolchain_name_for_host("x86_64-unknown-linux-gnu"),
            "clippy".into(),
            "rustfmt".into(),
        ],
        deadline,
    )?;
    verify_rust_tree(temp, catalog, "verify-scratch", deadline)
}

fn verify_tool_bytes(native: &Path) -> Result<(), OrchestratorError> {
    let tools = native.join("native-tools");
    for (name, digest) in [
        (
            "mise",
            velnor_actions_contract::NATIVE_MISE_LINUX_AMD64_BINARY_SHA256,
        ),
        (
            "mbx",
            velnor_actions_contract::NATIVE_MBX_LINUX_AMD64_BINARY_SHA256,
        ),
    ] {
        if super::build::support::hash_regular(&tools.join(name), MAX_NATIVE_TOOL_BYTES)? != digest
        {
            return Err(internal("native_qualification_bootstrap_bytes"));
        }
    }
    Ok(())
}

pub(super) fn verify_after_source(
    root: &Path,
    temp: &Path,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let root = source::checked_root(root)?;
    let source_commit = source::env("GITHUB_SHA")?;
    validate::qualification_identity(&source_commit)?;
    let head = source::git_text(
        &root,
        &velnor_actions_mise::GitRequest::rev_parse(vec!["HEAD".into()]),
        deadline,
    )?;
    if head.trim() != source_commit {
        return Err(internal("native_qualification_source_identity"));
    }
    if source::env("RUNNER_TEMP")? != path(temp)?
        || source::env("GITHUB_WORKSPACE")? != path(&root)?
    {
        return Err(internal("native_qualification_runner_identity"));
    }
    source::ensure_clean_checkout(&root, deadline)?;
    let config = crate::config::load_config(&root)?;
    let catalog = crate::tool_catalog_resolve::resolve_tool_catalog(&root, &config)?;
    verify_tool_bytes(&temp.join("velnor-qualification-runtime"))?;
    verify_rust_tree(temp, &catalog, "verify-after-source", deadline)
}

fn verify_rust_tree(
    temp: &Path,
    catalog: &ToolCatalog,
    scratch_name: &str,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let native = temp.join("velnor-qualification-runtime");
    let home = native.join("bootstrap-home");
    let scratch = native.join(scratch_name);
    for dir in [&scratch, &scratch.join("velnor")] {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(dir)
            .map_err(|_| internal("native_qualification_private_directory"))?;
    }
    let mut verify_env = tool_env(&native, &home, temp, catalog, true)?;
    verify_env.insert("RUNNER_TEMP".into(), path(&scratch)?);
    verify_env.insert(
        "VELNOR_TOOL_RUST".into(),
        catalog.version(PinnedTool::Rust).to_owned(),
    );
    let verifier = verify_rust_owned_argv(catalog, ToolchainPhase::Installed);
    run_env(temp, &verify_env, "/bin/sh", &verifier[1..], deadline)
}

fn tool_env(
    native: &Path,
    home: &Path,
    temp: &Path,
    catalog: &ToolCatalog,
    disable_auto: bool,
) -> Result<BTreeMap<String, String>, OrchestratorError> {
    let rustup = path(&native.join("rustup"))?;
    let cargo = path(&native.join("cargo"))?;
    let mut env = BTreeMap::from([
        (
            "PATH".into(),
            format!("{}:/usr/bin:/bin", path(&native.join("native-tools"))?),
        ),
        ("HOME".into(), path(home)?),
        ("RUNNER_TEMP".into(), path(temp)?),
        ("MISE_DATA_DIR".into(), path(&native.join("mise"))?),
        ("MISE_CACHE_DIR".into(), path(&native.join("mise-cache"))?),
        ("MISE_RUSTUP_HOME".into(), rustup.clone()),
        ("RUSTUP_HOME".into(), rustup),
        ("MISE_CARGO_HOME".into(), cargo.clone()),
        ("CARGO_HOME".into(), cargo),
        ("MISE_NO_CONFIG".into(), "1".into()),
        ("MISE_NO_ENV".into(), "1".into()),
        ("MISE_NO_HOOKS".into(), "1".into()),
        ("MISE_LOCKFILE".into(), "0".into()),
    ]);
    if disable_auto {
        env.insert("MISE_AUTO_INSTALL".into(), "false".into());
        env.insert("MISE_EXEC_AUTO_INSTALL".into(), "false".into());
    }
    if let Some(toolchain) = catalog.rustup_toolchain_env() {
        env.insert("RUSTUP_TOOLCHAIN".into(), toolchain.to_owned());
    }
    Ok(env)
}

fn execute(
    root: &Path,
    temp: &Path,
    catalog: &ToolCatalog,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let inputs = velnor_actions_contract::JobImageCandidateInputs {
        rust: catalog.tool_spec(PinnedTool::Rust),
        mr_boxington: catalog.tool_spec(PinnedTool::MrBoxington),
        export_rustup: catalog.exports_rustup_toolchain(),
    };
    let argv = inputs.qualification_argv()?;
    let runner_temp = path(temp)?;
    let task_env = inputs
        .qualification_environment()?
        .into_iter()
        .map(|(key, value)| {
            format!(
                "{key}={}",
                value.replace("${{ runner.temp }}", &runner_temp)
            )
        })
        .collect::<Vec<_>>();
    let script = format!(
        "fail_frame() {{ exit 1; }}\ntask_env=()\nfor ((i=0; i<{}; i++)); do task_env+=(\"$1\"); shift; done\nargv=(\"$@\")\n{}",
        task_env.len(),
        native_execution_environment_script(NativeExecutionPurpose::Qualification)
    );
    let mut env = BTreeMap::new();
    for key in [
        "RUNNER_TEMP",
        "GITHUB_ENV",
        "GITHUB_WORKSPACE",
        "GITHUB_SHA",
        "GITHUB_WORKFLOW_SHA",
        "GITHUB_REPOSITORY",
        "GITHUB_REPOSITORY_ID",
        "GITHUB_EVENT_NAME",
        "GITHUB_WORKFLOW_REF",
        "GITHUB_RUN_ID",
        "GITHUB_RUN_ATTEMPT",
        "GITHUB_REF",
    ] {
        env.insert(key.to_owned(), source::env(key)?);
    }
    if let Ok(value) = std::env::var("GITHUB_REF_PROTECTED") {
        env.insert("GITHUB_REF_PROTECTED".into(), value);
    }
    if let Ok(value) = std::env::var("RUSTFLAGS") {
        env.insert("RUSTFLAGS".into(), value);
    }
    if env.get("RUNNER_TEMP") != Some(&path(temp)?)
        || env.get("GITHUB_WORKSPACE") != Some(&path(root)?)
    {
        return Err(internal("native_qualification_runner_identity"));
    }
    let mut arguments = vec![
        "--noprofile".into(),
        "--norc".into(),
        "-p".into(),
        "-e".into(),
        "-o".into(),
        "pipefail".into(),
        "-c".into(),
        script,
        "velnor-native-qualification".into(),
    ];
    arguments.extend(task_env);
    arguments.extend(argv);
    run_env(root, &env, "/bin/bash", &arguments, deadline)
}

fn run_env(
    root: &Path,
    env: &BTreeMap<String, String>,
    program: &str,
    arguments: &[String],
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let mut command_args = vec![OsString::from("-i")];
    command_args.extend(
        env.iter()
            .map(|(key, value)| OsString::from(format!("{key}={value}"))),
    );
    command_args.push(program.into());
    command_args.extend(arguments.iter().map(OsString::from));
    let command = IsolatedCommand::repo_task("/usr/bin/env", command_args, &[])
        .map_err(|_| internal("native_qualification_command"))?
        .with_cwd(root.to_path_buf());
    let (output, _) = command
        .run_quiescent_until(2 * 1024 * 1024, deadline)
        .map_err(|error| internal(&format!("native_qualification_execution:{error}")))?;
    output
        .require_success("native qualification")
        .map_err(|error| internal(&format!("native_qualification_failed:{error}")))?;
    if program == "/bin/bash" {
        emit_archive_reuse(&output.stdout)?;
    }
    Ok(())
}

fn emit_archive_reuse(bytes: &[u8]) -> Result<(), OrchestratorError> {
    let mut seen = [false; 2];
    for raw in bytes.split(|byte| *byte == b'\n') {
        let Ok(line) = std::str::from_utf8(raw) else {
            continue;
        };
        if !line.starts_with("velnor: native archive reuse ") {
            continue;
        }
        let words = line.split(' ').collect::<Vec<_>>();
        let ["velnor:", "native", "archive", "reuse", mode, count, size] = words.as_slice() else {
            return Err(internal("native_archive_reuse_diagnostic"));
        };
        let slot = match *mode {
            "mode=workspace" => 0,
            "mode=fixture" => 1,
            _ => return Err(internal("native_archive_reuse_diagnostic")),
        };
        let count = count
            .strip_prefix("copied=")
            .filter(|value| canonical_number(value))
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|value| *value <= 4096)
            .ok_or_else(|| internal("native_archive_reuse_diagnostic"))?;
        let size = size
            .strip_prefix("bytes=")
            .filter(|value| canonical_number(value))
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value <= 1024 * 1024 * 1024)
            .ok_or_else(|| internal("native_archive_reuse_diagnostic"))?;
        if seen[slot] || (count == 0) != (size == 0) {
            return Err(internal("native_archive_reuse_diagnostic"));
        }
        seen[slot] = true;
        println!("velnor: native archive reuse {mode} copied={count} bytes={size}");
    }
    if seen != [true, true] {
        return Err(internal("native_archive_reuse_diagnostic"));
    }
    Ok(())
}

fn canonical_number(value: &str) -> bool {
    !value.is_empty()
        && (value == "0" || !value.starts_with('0'))
        && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn path(path: &Path) -> Result<String, OrchestratorError> {
    path.to_str()
        .filter(|value| !value.is_empty() && !value.contains(['\n', '\r', '\0']))
        .map(str::to_owned)
        .ok_or_else(|| internal("native_qualification_path"))
}

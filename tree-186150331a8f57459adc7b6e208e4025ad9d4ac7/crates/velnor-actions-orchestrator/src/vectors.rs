//! V1 fixed command vectors built only through the Mise adapter.

use std::ffi::OsString;

use velnor_actions_mise::{PinnedTool, PinnedToolExec, ToolCatalog};
use velnor_actions_rust::{TaskGroup, TaskKind};
use velnor_actions_workflow_renderer::render::CandidateSpec;

use crate::OrchestratorError;

/// V1 fixed vector for one group: pinned `mise` payload plus kind args.
pub(crate) fn task_argv(
    group: &TaskGroup,
    catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    let mut tools = vec![PinnedTool::Rust];
    if group.compile_driver == "mbx" {
        tools.push(PinnedTool::MrBoxington);
    }
    let manifest = manifest_for_key(&group.manifest_key);
    let mut args: Vec<OsString> = Vec::new();
    push_kind_args(&mut args, group, &manifest);
    push_feature_args(&mut args, group);
    if group.target != "host" {
        args.push(OsString::from("--target"));
        args.push(OsString::from(&group.target));
    }
    let program = OsString::from("cargo");
    let exec =
        PinnedToolExec::new(tools, &program, args).map_err(|err| OrchestratorError::Contract {
            problem: err.to_string(),
        })?;
    strings_of(exec.argv(catalog)).map_err(|problem| OrchestratorError::Contract { problem })
}

/// Fixed policy-job vector: a pinned `gh` version probe.
pub(crate) fn verify_tools_argv(catalog: &ToolCatalog) -> Result<Vec<String>, OrchestratorError> {
    let program = OsString::from("gh");
    let exec = PinnedToolExec::new(
        vec![PinnedTool::Gh],
        &program,
        vec![OsString::from("--version")],
    )
    .map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    strings_of(exec.argv(catalog)).map_err(|problem| OrchestratorError::Contract { problem })
}

/// Fixed candidate build plus qualification vectors.
pub(crate) fn candidate_spec(catalog: &ToolCatalog) -> Result<CandidateSpec, OrchestratorError> {
    let mbx = OsString::from("mbx");
    let build = PinnedToolExec::new(
        vec![PinnedTool::Rust, PinnedTool::MrBoxington],
        &mbx,
        fixed(&[
            "build",
            "--release",
            "--locked",
            "--package",
            "velnor-actions-cli",
            "--bin",
            "velnor-actions",
        ]),
    )
    .map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    let cargo = OsString::from("cargo");
    let qualify = PinnedToolExec::new(
        vec![PinnedTool::Rust],
        &cargo,
        fixed(&[
            "test",
            "--locked",
            "--offline",
            "--package",
            "velnor-actions-cli",
        ]),
    )
    .map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    Ok(CandidateSpec {
        build: strings_of(build.argv(catalog))
            .map_err(|problem| OrchestratorError::Contract { problem })?,
        qualify: strings_of(qualify.argv(catalog))
            .map_err(|problem| OrchestratorError::Contract { problem })?,
    })
}

/// Append the per-kind fixed payload arguments.
fn push_kind_args(args: &mut Vec<OsString>, group: &TaskGroup, manifest: &str) {
    let flag = OsString::from;
    match group.kind {
        TaskKind::Fmt => {
            args.extend([
                flag("fmt"),
                flag("--check"),
                flag("--manifest-path"),
                flag(manifest),
            ]);
        }
        TaskKind::Clippy => {
            args.extend([
                flag("clippy"),
                flag("--locked"),
                flag("--offline"),
                flag("--manifest-path"),
                flag(manifest),
            ]);
            if !group.package_name.is_empty() {
                args.extend([flag("--package"), flag(&group.package_name)]);
            }
            args.push(flag("--all-targets"));
        }
        TaskKind::Test => {
            args.extend([flag("test"), flag("--locked"), flag("--offline")]);
            push_manifest(args, manifest);
            for target_flag in &group.target_flags {
                args.push(flag(target_flag));
            }
        }
        TaskKind::Nextest => {
            args.extend([
                flag("nextest"),
                flag("run"),
                flag("--locked"),
                flag("--offline"),
            ]);
            push_manifest(args, manifest);
        }
        TaskKind::Doctest => {
            args.extend([flag("test"), flag("--locked"), flag("--offline")]);
            push_manifest(args, manifest);
            args.push(flag("--doc"));
        }
        TaskKind::Doc => {
            args.extend([flag("doc"), flag("--locked"), flag("--offline")]);
            push_manifest(args, manifest);
            args.push(flag("--no-deps"));
        }
        TaskKind::Build => {
            args.extend([flag("build"), flag("--locked"), flag("--offline")]);
            push_manifest(args, manifest);
        }
    }
}

/// Append `--manifest-path <manifest>`.
fn push_manifest(args: &mut Vec<OsString>, manifest: &str) {
    args.push(OsString::from("--manifest-path"));
    args.push(OsString::from(manifest));
}

/// Append feature flags unless the group uses default features.
fn push_feature_args(args: &mut Vec<OsString>, group: &TaskGroup) {
    if group.kind == TaskKind::Fmt {
        return;
    }
    if group.features.len() == 1 && group.features[0] == "default" {
        return;
    }
    args.push(OsString::from("--no-default-features"));
    if !group.features.is_empty() {
        args.push(OsString::from("--features"));
        args.push(OsString::from(group.features.join(",")));
    }
}

/// Manifest path for a manifest key.
fn manifest_for_key(key: &str) -> String {
    if key == "root" {
        "Cargo.toml".to_owned()
    } else {
        format!("{key}/Cargo.toml")
    }
}

/// Build a fixed argument list.
fn fixed(flags: &[&str]) -> Vec<OsString> {
    flags.iter().map(OsString::from).collect()
}

/// Convert fixed argv to UTF-8 strings.
fn strings_of(argv: Vec<OsString>) -> Result<Vec<String>, String> {
    let mut out = Vec::with_capacity(argv.len());
    for arg in argv {
        match arg.into_string() {
            Ok(text) => out.push(text),
            Err(_) => return Err("non_utf8_argv".to_owned()),
        }
    }
    Ok(out)
}

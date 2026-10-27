// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::ExitStatus,
};

use velnor_actions_mise::{PinnedTool, ToolCatalog};

#[test]
fn public_generation_keeps_plan_bootstrap_and_uses_required_verdict_controller()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let paths = super::FixturePaths::new(&temp);
    paths.create("import")?;
    let cli = PathBuf::from(env!("CARGO_BIN_EXE_velnor-actions"));
    status(
        super::run_cli_generate(&cli, &paths)?,
        "fixture generation failed",
    )?;
    let workflow = fs::read_to_string(paths.repo.join(".github/workflows/ci.yml"))?;
    let plan = workflow_job(&workflow, "plan")?;
    assert!(
        plan.contains("/v0.1.55/velnor-actions-0.1.55-x86_64-unknown-linux-gnu"),
        "Plan no longer uses the immutable SDK55 bootstrap"
    );
    assert!(plan.contains("gh@2.103.2"), "Plan helper catalog changed");
    assert!(!plan.contains("velnor-actions-0.1.59"));
    let required = workflow_job(&workflow, "required")?;
    assert!(
        required.contains("/v0.1.59/velnor-actions-0.1.59-x86_64-unknown-linux-gnu"),
        "Required verdict does not acquire the authenticated SDK59 controller"
    );
    assert!(required.contains("717be00d245f511bd5d324574cb80b72a34c5a83c639fafa6af1c0acb8649b26"));
    assert!(required.contains("4bb724119971fe49f07b12876c54c3c159edb5e5"));
    assert!(
        required.contains("gh@2.103.3"),
        "Required helper catalog changed"
    );
    assert_required_pipeline(&paths)?;
    assert!(!workflow.contains("\n  candidate:"));
    Ok(())
}

#[test]
fn consumer_required_uses_canonical_group_ledger_fanin() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let paths = super::FixturePaths::new(&temp);
    paths.create("import")?;
    let config_path = paths.repo.join(".velnor/config.toml");
    let config = fs::read_to_string(&config_path)?;
    let consumer = config.replace(
        "policy = \"velnor-repository-v1\"",
        "policy = \"consumer-v1\"",
    );
    assert_ne!(config, consumer, "fixture policy was not Velnor-specific");
    fs::write(config_path, consumer)?;
    let jobs = super::finalized_jobs(&super::prepare(&paths.repo)?)?;
    let required = jobs
        .get(super::REQUIRED_JOB_ID)
        .ok_or("Required job missing")?;
    assert_required_ledger_order(required)
}

fn assert_required_pipeline(paths: &super::FixturePaths) -> Result<(), Box<dyn Error>> {
    let jobs = super::finalized_jobs(&super::prepare(&paths.repo)?)?;
    let required = jobs
        .get(super::REQUIRED_JOB_ID)
        .ok_or("Required job missing")?;
    assert_required_ledger_order(required)?;
    let setup = required
        .steps
        .iter()
        .find(|step| step.name == "Setup Mise")
        .ok_or("Required Mise setup missing")?;
    let super::StepKind::Action { with, .. } = &setup.kind else {
        return Err("Required Mise setup was not an action".into());
    };
    assert_eq!(
        with.get("version").map(String::as_str),
        Some(velnor_actions_mise::MISE_VERSION),
        "Required verdict does not use the current Mise pin"
    );
    Ok(())
}

fn assert_required_ledger_order(
    required: &velnor_actions_contract::Job,
) -> Result<(), Box<dyn Error>> {
    let names = [
        "Download plan",
        "Download every expected matrix artifact",
        "Assemble group ledgers",
        "Write request",
        "Merge reports",
    ];
    let positions = names
        .iter()
        .map(|name| {
            required
                .steps
                .iter()
                .position(|step| step.name == *name)
                .ok_or_else(|| std::io::Error::other(format!("Required step missing: {name}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "Required ledger fan-in must follow download plan, fetch reports, assemble ledgers, write request, and merge"
    );
    assert_internal_operation(
        required,
        "Assemble group ledgers",
        "assemble-group-ledgers-v1",
    )?;
    assert_internal_operation(required, "Write request", "write-request-v1:merge-v1")?;
    assert_internal_operation(required, "Merge reports", "merge-v1")?;
    Ok(())
}

fn assert_internal_operation(
    job: &velnor_actions_contract::Job,
    step_name: &str,
    expected_operation: &str,
) -> Result<(), Box<dyn Error>> {
    let step = job
        .steps
        .iter()
        .find(|step| step.name == step_name)
        .ok_or_else(|| std::io::Error::other(format!("Required step missing: {step_name}")))?;
    let super::StepKind::Internal { operation, .. } = &step.kind else {
        return Err(format!("Required step was not an internal operation: {step_name}").into());
    };
    assert_eq!(operation, expected_operation);
    Ok(())
}

fn workflow_job(workflow: &str, job_id: &str) -> Result<String, Box<dyn Error>> {
    let heading = format!("  {job_id}:");
    let mut found = false;
    let mut job = String::new();
    for line in workflow.lines() {
        if found && line.starts_with("  ") && !line.starts_with("    ") {
            break;
        }
        if line == heading {
            found = true;
        }
        if found {
            job.push_str(line);
            job.push('\n');
        }
    }
    if !found {
        return Err(format!("generated workflow omitted job {job_id}").into());
    }
    Ok(job)
}

const MISE_TRANSPORT: &str = r#"#!/bin/sh
set -eu
[ "$#" -ge 4 ] || exit 90
[ "$1" = "--no-config" ] || exit 91
[ "$2" = "--no-env" ] || exit 92
[ "$3" = "--no-hooks" ] || exit 93
shift 3
[ "$1" = "exec" ] || exit 94
shift
first="$1"
shift
if [ "$first" = "@JACTIONLINT_SPEC@" ]; then
  [ "$1" = "@SHELLCHECK_SPEC@" ] || exit 95
  shift
elif [ "$first" != "@SHELLCHECK_SPEC@" ]; then
  exit 96
fi
[ "$1" = "--" ] || exit 97
shift
program="$1"
shift
case "$program" in
  jactionlint)
    [ "$first" = "@JACTIONLINT_SPEC@" ] || exit 98
    [ "$1" = "--config-file" ] && [ "$2" = ".github/jactionlint.yaml" ] || exit 99
    [ "$3" = "--format" ] && [ "$4" = "oneline" ] || exit 100
    shift 4
    for flag in --strict-exit --no-online --shellcheck=shellcheck --pyflakes= --no-color; do
      [ "$1" = "$flag" ] || exit 101
      shift
    done
    [ "$#" -gt 0 ] || exit 102
    for file in "$@"; do [ -f "$file" ] || exit 103; done
    exit 0
    ;;
  shellcheck)
    [ "$first" = "@SHELLCHECK_SPEC@" ] || exit 104
    if [ "$1" = "--version" ]; then
      [ "$#" -eq 1 ] || exit 105
      printf 'version: %s\n' "@SHELLCHECK_VERSION@"
      exit 0
    fi
    if [ "$1" = "-S" ]; then
      [ "$2" = "warning" ] && [ "$3" = "--format=gcc" ] || exit 106
      shift 3
    elif [ "$1" = "--format=gcc" ]; then
      [ "$2" = "--include=SC2086" ] || exit 107
      shift 2
    else
      exit 108
    fi
    [ "$#" -gt 0 ] || exit 109
    for file in "$@"; do [ -f "$file" ] || exit 110; done
    exit 0
    ;;
  *) exit 111 ;;
esac
"#;

const MBX_TRANSPORT: &str = r#"#!/bin/sh
set -eu
[ "${MBX_CARGO_SHIM_MODE:-}" = "1" ] || exit 119
[ "$#" -eq 13 ] || exit 120
[ "$1" = "--config" ] || exit 122
[ "$2" = "build.jobs=2" ] || exit 123
[ "$3" = "run" ] || exit 124
[ "$4" = "--quiet" ] || exit 125
[ "$5" = "--locked" ] || exit 126
[ "$6" = "--package" ] && [ "$7" = "velnor-actions-cli" ] || exit 127
[ "$8" = "--bin" ] && [ "$9" = "velnor-actions" ] || exit 128
[ "${10}" = "--" ] && [ "${11}" = "generate" ] || exit 129
[ "${12}" = "--output-dir" ] || exit 130
expected="${RUNNER_TEMP:?}/velnor-actions-freshness-${GITHUB_RUN_ID:?}-${GITHUB_RUN_ATTEMPT:?}/generated"
[ "${13}" = "$expected" ] || exit 131
exec "${VELNOR_ACTIONS_BINARY:?}" generate --output-dir "${13}"
"#;

pub(super) fn write_mise_transport(bin: &Path) -> Result<(), Box<dyn Error>> {
    let catalog = ToolCatalog::pinned();
    let source = MISE_TRANSPORT
        .replace(
            "@JACTIONLINT_SPEC@",
            &catalog.tool_spec(PinnedTool::Jactionlint),
        )
        .replace(
            "@SHELLCHECK_SPEC@",
            &catalog.tool_spec(PinnedTool::Shellcheck),
        )
        .replace(
            "@SHELLCHECK_VERSION@",
            catalog.version(PinnedTool::Shellcheck),
        );
    write_executable(bin.join("mise"), source.as_bytes())
}

pub(super) fn write_mbx_transport(bin: &Path) -> Result<(), Box<dyn Error>> {
    write_executable(bin.join("mbx"), MBX_TRANSPORT.as_bytes())
}

pub(super) fn status(status: ExitStatus, message: &str) -> Result<(), Box<dyn Error>> {
    if status.success() {
        Ok(())
    } else {
        Err(format!("{message}: {status}").into())
    }
}

fn write_executable(path: impl AsRef<Path>, content: &[u8]) -> Result<(), Box<dyn Error>> {
    fs::write(&path, content)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

use std::{
    error::Error,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use velnor_actions_contract::{StepKind, StepRole};
use velnor_actions_mise::{PinnedTool, ToolCatalog, ToolHomes};

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
    [ "$1" = "--config-file" ] || exit 99
    [ "$2" = ".github/jactionlint.yaml" ] || exit 100
    [ "$3" = "--format" ] || exit 101
    [ "$4" = "oneline" ] || exit 102
    shift 4
    for flag in --strict-exit --no-online --shellcheck=shellcheck --pyflakes= --no-color; do
      [ "$1" = "$flag" ] || exit 103
      shift
    done
    [ "$#" -gt 0 ] || exit 104
    for file in "$@"; do [ -f "$file" ] || exit 105; done
    exit 0
    ;;
  shellcheck)
    [ "$first" = "@SHELLCHECK_SPEC@" ] || exit 106
    if [ "$1" = "--version" ]; then
      [ "$#" -eq 1 ] || exit 107
      printf 'version: %s\n' "@SHELLCHECK_VERSION@"
      exit 0
    fi
    if [ "$1" = "-S" ]; then
      [ "$2" = "warning" ] || exit 108
      [ "$3" = "--format=gcc" ] || exit 109
      shift 3
    elif [ "$1" = "--format=gcc" ]; then
      [ "$2" = "--include=SC2086" ] || exit 110
      shift 2
    else
      exit 111
    fi
    [ "$#" -gt 0 ] || exit 112
    for file in "$@"; do [ -f "$file" ] || exit 113; done
    exit 0
    ;;
  *) exit 114 ;;
esac
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
    let path = bin.join("mise");
    fs::write(&path, source)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

pub(super) fn freshness_preview(runner_temp: &Path) -> PathBuf {
    runner_temp.join("velnor-actions-501-1")
}

pub(super) fn read_generated_workflow(runner_temp: &Path) -> Result<Vec<u8>, Box<dyn Error>> {
    Ok(fs::read(generated_workflow(runner_temp))?)
}

pub(super) fn assert_generated_claude_link(runner_temp: &Path) -> Result<(), Box<dyn Error>> {
    let path = freshness_preview(runner_temp).join(".github/CLAUDE.md");
    if !fs::symlink_metadata(&path)?.file_type().is_symlink()
        || fs::read_link(path)? != Path::new("AGENTS.md")
    {
        return Err("generated Claude file is not the canonical symlink".into());
    }
    Ok(())
}

pub(super) fn reset_freshness_preview(runner_temp: &Path) -> Result<(), Box<dyn Error>> {
    let preview = freshness_preview(runner_temp);
    let metadata = fs::symlink_metadata(&preview)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("freshness preview is not a real directory".into());
    }
    let root = fs::canonicalize(runner_temp)?;
    let canonical = fs::canonicalize(&preview)?;
    if !canonical.starts_with(root) {
        return Err("freshness preview escaped runner temp".into());
    }
    fs::remove_dir_all(canonical)?;
    Ok(())
}

pub(super) fn generated_workflow(runner_temp: &Path) -> PathBuf {
    freshness_preview(runner_temp).join(".github/workflows/ci.yml")
}

pub(super) fn expected_mutation(changes_workflow: bool, before: &[u8]) -> Vec<u8> {
    if changes_workflow {
        [before, b"\n# freshness mutation\n"].concat()
    } else {
        before.to_vec()
    }
}

pub(super) fn assert_unchanged(path: &Path, expected: &[u8]) -> Result<(), Box<dyn Error>> {
    assert_eq!(fs::read(path)?, expected, "freshness check repaired input");
    Ok(())
}

pub(super) fn assert_regular_file(path: &Path) -> Result<(), Box<dyn Error>> {
    if !fs::metadata(path)?.is_file() || fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err("freshness input was not preserved as a regular file".into());
    }
    Ok(())
}

pub(super) fn assert_candidate_tools(
    candidate: &velnor_actions_contract::Job,
) -> Result<(), Box<dyn Error>> {
    let provision = candidate
        .steps
        .iter()
        .position(|step| step.role == Some(StepRole::PreparePinnedTools))
        .ok_or("candidate tool provisioning missing")?;
    let build = candidate
        .steps
        .iter()
        .position(|step| step.name == "Build candidate")
        .ok_or("candidate build step missing")?;
    assert!(provision < build);
    let expected_specs = [
        PinnedTool::Rust,
        PinnedTool::MrBoxington,
        PinnedTool::Jactionlint,
        PinnedTool::Shellcheck,
    ]
    .map(|tool| ToolCatalog::pinned().tool_spec(tool));
    let StepKind::Shell {
        run: install,
        env: install_env,
    } = &candidate.steps[provision].kind
    else {
        return Err("candidate tool provisioning is not a shell step".into());
    };
    for spec in expected_specs {
        assert!(install.iter().any(|arg| arg == &spec));
    }
    let restored = verifier_action_position(
        candidate,
        velnor_actions_contract::workflow::RUST_VERIFIER_USES[0],
    )?;
    let jactionlint = verifier_action_position(
        candidate,
        velnor_actions_contract::workflow::JACTIONLINT_VERIFIER_USES,
    )?;
    let installed = verifier_action_position(
        candidate,
        velnor_actions_contract::workflow::RUST_VERIFIER_USES[1],
    )?;
    assert!(restored < provision && provision < jactionlint);
    assert!(jactionlint < installed && installed < build);
    let StepKind::Shell { env: build_env, .. } = &candidate.steps[build].kind else {
        return Err("candidate build step is not a shell step".into());
    };
    let homes = ToolHomes::runner_temp();
    for key in ["MISE_RUSTUP_HOME", "RUSTUP_HOME"] {
        assert_eq!(
            install_env.get(key).map(String::as_str),
            Some(homes.rustup_home()),
            "Prepare pinned tools has the wrong {key}"
        );
        assert_eq!(
            build_env.get(key).map(String::as_str),
            Some(homes.rustup_home()),
            "Build candidate has the wrong {key}"
        );
    }
    for key in ["MISE_CARGO_HOME", "CARGO_HOME"] {
        assert_eq!(
            install_env.get(key).map(String::as_str),
            Some(homes.cargo_home()),
            "Prepare pinned tools has the wrong {key}"
        );
        assert_eq!(
            build_env.get(key).map(String::as_str),
            Some(homes.cargo_home()),
            "Build candidate has the wrong {key}"
        );
    }
    Ok(())
}

fn verifier_action_position(
    job: &velnor_actions_contract::Job,
    uses: &str,
) -> Result<usize, Box<dyn Error>> {
    job.steps
        .iter()
        .position(|step| {
            matches!(&step.kind, StepKind::Action { uses: actual, with, env }
                if actual == uses && with.is_empty() && env.is_empty())
        })
        .ok_or_else(|| format!("candidate verifier action missing: {uses}").into())
}

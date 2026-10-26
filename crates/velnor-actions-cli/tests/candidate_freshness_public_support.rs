// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs, os::unix::fs::PermissionsExt, path::Path, process::ExitStatus};

use velnor_actions_mise::{PinnedTool, ToolCatalog};

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

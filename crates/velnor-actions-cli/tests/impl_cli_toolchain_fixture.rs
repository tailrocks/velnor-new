//! Exact local fixture for Rust-pin presence in workflow-rendering tests.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use velnor_actions_mise::{JACTIONLINT_VERSION, SHELLCHECK_VERSION};

/// Satisfy the exact Rust inventory query and forward only the pinned
/// metadata request used by CLI planning to the real Cargo executable.
#[cfg(unix)]
pub(super) fn installed_rust_mise_fixture(root: &Path) -> Result<PathBuf, Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;

    let real_mise = shell_quote(&resolve_mise()?)?;
    let bin = root.join("fixture-bin");
    fs::create_dir_all(&bin)?;
    let mise = bin.join("mise");
    let jactionlint_version = JACTIONLINT_VERSION;
    let shellcheck_version = SHELLCHECK_VERSION;
    let script = format!(
        r#"#!/bin/sh
set -eu
REAL_MISE={real_mise}
if [ "$#" -eq 1 ] && [ "$1" = --version ]; then
  exec "$REAL_MISE" --version
fi
[ "$#" -ge 4 ] || exit 90
[ "$1" = --no-config ] || exit 91
[ "$2" = --no-env ] || exit 92
[ "$3" = --no-hooks ] || exit 93
shift 3
case "$1" in
  ls)
    [ "$#" -eq 3 ] || exit 94
    [ "$2" = --json ] || exit 95
    [ "$3" = rust ] || exit 96
    printf '%s\n' '[{{"version":"1.99.1","installed":true}}]'
    ;;
  exec)
    [ "$#" -ge 5 ] || exit 97
    case "$2" in
      rust@1.99.1)
        [ "$#" -eq 10 ] || exit 98
        [ "$3" = -- ] || exit 99
        shift 3
        [ "$#" -eq 7 ] || exit 100
        [ "$1" = cargo ] || exit 101
        [ "$2" = metadata ] || exit 102
        [ "$3" = --format-version ] || exit 103
        [ "$4" = 1 ] || exit 104
        [ "$5" = --no-deps ] || exit 105
        [ "$6" = --manifest-path ] || exit 106
        case "$7" in
          /*/Cargo.toml) [ -f "$7" ] || exit 107 ;;
          *) exit 107 ;;
        esac
        exec "$@"
        ;;
      jactionlint@{jactionlint_version})
        [ "$#" -ge 15 ] || exit 108
        [ "$3" = shellcheck@{shellcheck_version} ] || exit 109
        [ "$4" = -- ] || exit 110
        [ "$5" = jactionlint ] || exit 111
        [ "$6" = --config-file ] || exit 112
        [ "$7" = .github/jactionlint.yaml ] || exit 113
        [ "$8" = --format ] || exit 114
        [ "$9" = oneline ] || exit 115
        [ "${{10}}" = --strict-exit ] || exit 116
        [ "${{11}}" = --no-online ] || exit 117
        [ "${{12}}" = --shellcheck=shellcheck ] || exit 118
        [ "${{13}}" = --pyflakes= ] || exit 119
        [ "${{14}}" = --no-color ] || exit 120
        exec "$REAL_MISE" --no-config --no-env --no-hooks "$@"
        ;;
      shellcheck@{shellcheck_version})
        [ "$3" = -- ] || exit 121
        [ "$4" = shellcheck ] || exit 122
        case "$5" in
          --version)
            [ "$#" -eq 5 ] || exit 123
            ;;
          -S)
            [ "$#" -ge 8 ] || exit 124
            [ "$6" = warning ] || exit 125
            [ "$7" = --format=gcc ] || exit 126
            ;;
          --format=gcc)
            [ "$#" -ge 7 ] || exit 127
            [ "$6" = --include=SC2086 ] || exit 128
            ;;
          *)
            exit 129
            ;;
        esac
        exec "$REAL_MISE" --no-config --no-env --no-hooks "$@"
        ;;
      *)
        exit 129
        ;;
    esac
    ;;
  *)
    exit 130
    ;;
esac
"#
    );
    fs::write(&mise, script)?;
    fs::set_permissions(&mise, fs::Permissions::from_mode(0o755))?;
    Ok(bin)
}

fn resolve_mise() -> Result<PathBuf, Box<dyn Error>> {
    let path = std::env::var_os("PATH").ok_or("PATH is unset")?;
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join("mise");
        if candidate.is_file() {
            return Ok(candidate.canonicalize()?);
        }
    }
    Err("mise executable not found on the test process PATH".into())
}

fn shell_quote(path: &Path) -> Result<String, Box<dyn Error>> {
    let path = path.to_str().ok_or("mise path is not UTF-8")?;
    Ok(format!("'{}'", path.replace('\'', "'\"'\"'")))
}

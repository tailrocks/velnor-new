//! Exact local fixture for Rust-pin presence in workflow-rendering tests.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

const JACTIONLINT_VERSION: &str = "2.3.0";
const SHELLCHECK_VERSION: &str = "0.11.0";
const MISE_SHIM: &str = include_str!("fixtures/installed-rust-mise.sh");

/// Satisfy the exact Rust inventory query and forward only the pinned
/// metadata request used by CLI planning to the real Cargo executable.
#[cfg(unix)]
pub(super) fn installed_rust_mise_fixture(root: &Path) -> Result<PathBuf, Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;

    let real_mise = shell_quote(&resolve_mise()?)?;
    let bin = root.join("fixture-bin");
    fs::create_dir_all(&bin)?;
    let mise = bin.join("mise");
    let script = MISE_SHIM
        .replace("@JACTIONLINT_VERSION@", JACTIONLINT_VERSION)
        .replace("@SHELLCHECK_VERSION@", SHELLCHECK_VERSION)
        .replace("@REAL_MISE@", &real_mise);
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

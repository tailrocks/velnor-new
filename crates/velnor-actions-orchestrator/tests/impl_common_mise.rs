//! A real Mise delegate installed under the Setup Mise path for resolver tests.

use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub(crate) fn write_fixture_mise(data_dir: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let bin = data_dir.join("bin");
    fs::create_dir_all(&bin)?;
    let real_mise = mise_on_path()?;
    let mut body = String::from("#!/bin/sh\nset -eu\n");
    body.push_str("if [ -n \"${AUDIT-}\" ]; then\n");
    body.push_str("printf 'mise=%s\\nargs=%s\\n' \"$0\" \"$*\" >> \"$AUDIT\"\n");
    body.push_str("printf 'MISE_DATA_DIR=%s\\nPATH=%s\\n' \"${MISE_DATA_DIR-unset}\" \"$PATH\" >> \"$AUDIT\"\n");
    for name in [
        "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_URL",
        "ACTIONS_RUNTIME_TOKEN",
        "GITHUB_TOKEN",
        "MISE_GITHUB_TOKEN",
        "GH_TOKEN",
        "GH_HOST",
        "GH_CONFIG_DIR",
        "CARGO_REGISTRY_TOKEN",
        "NPM_TOKEN",
        "NODE_AUTH_TOKEN",
    ] {
        writeln!(
            body,
            "printf '{name}=%s\\n' \"${{{name}-<unset>}}\" >> \"$AUDIT\""
        )?;
    }
    body.push_str("fi\nexec ");
    body.push_str(&shell_quote(&real_mise.to_string_lossy()));
    body.push_str(" \"$@\"\n");

    let path = bin.join("mise");
    fs::write(&path, body)?;
    let mut permissions = fs::metadata(&path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions)?;
    Ok(data_dir.to_owned())
}

fn mise_on_path() -> Result<PathBuf, Box<dyn Error>> {
    let path = std::env::var_os("PATH").ok_or("PATH is unset")?;
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join("mise");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err("Mise is not available on the test PATH".into())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

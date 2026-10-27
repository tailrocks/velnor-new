// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::error::Error;
use std::fs;
use std::io;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) static NEXT_SCRATCH: AtomicU64 = AtomicU64::new(0);

pub(crate) const PREVIOUS_RUSTC_ENV: &str = "MBX_PREVIOUS_RUSTC_WRAPPER";
pub(crate) const PREVIOUS_RUSTDOC_ENV: &str = "MBX_PREVIOUS_RUSTDOC";
pub(crate) const PREVIOUS_CC_ENV: &str = "VELNOR_PREVIOUS_CC";
pub(crate) const PREVIOUS_CXX_ENV: &str = "VELNOR_PREVIOUS_CXX";

pub(crate) fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_velnor-actions"))
}

pub(crate) fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

pub(crate) fn scratch_root() -> Result<PathBuf, Box<dyn Error>> {
    let id = NEXT_SCRATCH.fetch_add(1, Ordering::SeqCst);
    let root = env::temp_dir().join(format!("velnor-cli-t29-{id}-{}", std::process::id()));
    fs::create_dir_all(&root).map_err(|error| fail(&format!("create scratch dir: {error}")))?;
    Ok(root)
}

pub(crate) fn cleanup(root: &Path) {
    if fs::remove_dir_all(root).is_err() {
        // Best effort: scratch cleanup must not fail the test.
    }
}

pub(crate) fn write_fake_compiler(
    dir: &Path,
    name: &str,
    tag: &str,
    log: &Path,
    exit: i32,
) -> Result<PathBuf, Box<dyn Error>> {
    let path = dir.join(name);
    let body = format!(
        "#!/bin/sh\nprintf '%s\\n' \"{tag}:$*\" >> \"{}\"\nexit {exit}\n",
        log.to_string_lossy()
    );
    fs::write(&path, body).map_err(|error| fail(&format!("write fake compiler: {error}")))?;
    let mut permissions = fs::metadata(&path)
        .map_err(|error| fail(&format!("stat fake compiler: {error}")))?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions)
        .map_err(|error| fail(&format!("chmod fake compiler: {error}")))?;
    Ok(path)
}

pub(crate) fn write_forwarding_wrapper(
    dir: &Path,
    name: &str,
    tag: &str,
    log: &Path,
) -> Result<PathBuf, Box<dyn Error>> {
    let path = dir.join(name);
    let body = format!(
        "#!/bin/sh\nprintf '%s\\n' \"{tag}:$*\" >> \"{}\"\nexec \"$@\"\n",
        log.to_string_lossy()
    );
    fs::write(&path, body).map_err(|error| fail(&format!("write forwarding wrapper: {error}")))?;
    let mut permissions = fs::metadata(&path)
        .map_err(|error| fail(&format!("stat forwarding wrapper: {error}")))?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions)
        .map_err(|error| fail(&format!("chmod forwarding wrapper: {error}")))?;
    Ok(path)
}

pub(crate) fn entry_link(dir: &Path, entry: &str) -> Result<PathBuf, Box<dyn Error>> {
    let link = dir.join(entry);
    symlink(binary(), &link).map_err(|error| fail(&format!("link observer entry: {error}")))?;
    Ok(link)
}

pub(crate) fn run_entry(
    entry: &Path,
    args: &[&str],
    extra_env: &[(&str, &str)],
) -> Result<Output, Box<dyn Error>> {
    let mut command = Command::new(entry);
    command.args(args);
    for key in [
        PREVIOUS_RUSTC_ENV,
        PREVIOUS_RUSTDOC_ENV,
        PREVIOUS_CC_ENV,
        PREVIOUS_CXX_ENV,
        "VELNOR_INTERNAL_OP",
    ] {
        command.env_remove(key);
    }
    for (key, value) in extra_env {
        command.env(key, value);
    }
    command
        .output()
        .map_err(|error| fail(&format!("run observer entry: {error}")))
}

pub(crate) fn read_log(log: &Path) -> Result<String, Box<dyn Error>> {
    fs::read_to_string(log).map_err(|error| fail(&format!("read compiler log: {error}")))
}

pub(crate) fn succeeded(output: &Output, what: &str) -> Result<(), Box<dyn Error>> {
    if output.status.success() {
        Ok(())
    } else {
        Err(fail(&format!(
            "{what} must succeed: {}",
            String::from_utf8_lossy(&output.stderr)
        )))
    }
}

pub(crate) fn rejected(output: &Output, what: &str) -> Result<(), Box<dyn Error>> {
    if output.status.success() {
        Err(fail(&format!("{what} must fail")))
    } else {
        Ok(())
    }
}

pub(crate) fn rejected_by_observer(output: &Output, what: &str) -> Result<(), Box<dyn Error>> {
    rejected(output, what)?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("compiler_observer_") {
        Ok(())
    } else {
        Err(fail(&format!(
            "{what} must be rejected by the observer layer"
        )))
    }
}

pub(crate) fn case_dir(root: &Path, name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir = root.join(name);
    fs::create_dir_all(&dir).map_err(|error| fail(&format!("create case dir: {error}")))?;
    Ok(dir)
}

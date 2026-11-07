// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, internal::internal};
use std::{fs, path::Path};

pub(crate) fn verify_toolchain_source_separation(source: &Path) -> Result<(), OrchestratorError> {
    let home = std::env::var_os("RUSTUP_HOME")
        .ok_or_else(|| internal("crate_release_toolchain_invalid"))?;
    let home = Path::new(&home)
        .canonicalize()
        .map_err(|_| internal("crate_release_toolchain_invalid"))?;
    let source = source
        .canonicalize()
        .map_err(|_| internal("crate_release_source_root_invalid"))?;
    if home.starts_with(&source) || source.starts_with(&home) {
        return Err(internal("crate_release_toolchain_source_overlap"));
    }
    Ok(())
}

pub(crate) fn source_compiler_env(
    version: &str,
    git: &crate::crate_release_git::ReleaseGit,
) -> Result<Vec<std::ffi::OsString>, OrchestratorError> {
    use velnor_actions_mise::release_git_pin::ReleaseGitExecutable as _;
    git.revalidate()
        .map_err(|_| internal("release_git_bundle_invalid"))?;
    let home = std::env::var_os("RUSTUP_HOME")
        .ok_or_else(|| internal("crate_release_toolchain_invalid"))?;
    let home = Path::new(&home);
    let rustc = verified_rust_tool_path(home, version, "rustc")?;
    let rustdoc = verified_rust_tool_path(home, version, "rustdoc")?;
    let bin = rustc
        .parent()
        .ok_or_else(|| internal("crate_release_toolchain_invalid"))?;
    let path = std::env::join_paths([
        git.exec_path(),
        bin,
        Path::new("/usr/bin"),
        Path::new("/bin"),
    ])
    .map_err(|_| internal("crate_release_toolchain_invalid"))?;
    let mut path_env = std::ffi::OsString::from("PATH=");
    path_env.push(path);
    let mut git_env = std::ffi::OsString::from("GIT_EXEC_PATH=");
    git_env.push(git.exec_path());
    Ok(vec![
        path_env,
        git_env,
        format!("RUSTC={}", rustc.display()).into(),
        format!("RUSTDOC={}", rustdoc.display()).into(),
    ])
}

pub(crate) fn source_cargo_path(
    rust: &str,
    _cwd: &Path,
) -> Result<std::path::PathBuf, OrchestratorError> {
    let home = std::env::var_os("RUSTUP_HOME")
        .ok_or_else(|| internal("crate_release_toolchain_invalid"))?;
    verified_rust_tool_path(Path::new(&home), rust, "cargo")
}

pub fn verified_rust_tool_path(
    rustup_home: &Path,
    version: &str,
    tool: &str,
) -> Result<std::path::PathBuf, OrchestratorError> {
    if !rustup_home.is_absolute()
        || !matches!(tool, "cargo" | "rustc" | "rustdoc")
        || !velnor_actions_contract::config::is_exact_semver(version)
    {
        return Err(internal("crate_release_toolchain_invalid"));
    }
    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        _ => return Err(internal("crate_release_toolchain_platform_unsupported")),
    };
    let path = rustup_home
        .join("toolchains")
        .join(format!("{version}-{target}"))
        .join("bin")
        .join(tool);
    let mut current = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::RootDir | std::path::Component::Normal(_) => {
                current.push(component);
            }
            _ => return Err(internal("crate_release_toolchain_invalid")),
        }
        let metadata = fs::symlink_metadata(&current)
            .map_err(|_| internal("crate_release_toolchain_invalid"))?;
        if metadata.file_type().is_symlink() {
            return Err(internal("crate_release_toolchain_invalid"));
        }
        if current != path && !metadata.is_dir() {
            return Err(internal("crate_release_toolchain_invalid"));
        }
        if current == path && !metadata.is_file() {
            return Err(internal("crate_release_toolchain_invalid"));
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if fs::metadata(&path)
            .map_err(|_| internal("crate_release_toolchain_invalid"))?
            .permissions()
            .mode()
            & 0o111
            == 0
        {
            return Err(internal("crate_release_toolchain_invalid"));
        }
    }
    Ok(path)
}

pub fn verify_owned_tool_homes(
    runner_temp: &Path,
    rustup_home: &Path,
    mise_rustup_home: &Path,
    cargo_home: &Path,
    mise_cargo_home: &Path,
    source: &Path,
) -> Result<(), OrchestratorError> {
    if !runner_temp.is_absolute()
        || rustup_home != runner_temp.join("velnor/rustup")
        || mise_rustup_home != rustup_home
        || cargo_home != runner_temp.join("velnor/cargo")
        || mise_cargo_home != cargo_home
    {
        return Err(internal("crate_release_owned_homes_invalid"));
    }
    for path in [runner_temp, rustup_home, cargo_home, source] {
        let mut current = std::path::PathBuf::new();
        for component in path.components() {
            match component {
                std::path::Component::RootDir | std::path::Component::Normal(_) => {
                    current.push(component);
                }
                _ => return Err(internal("crate_release_owned_homes_invalid")),
            }
            let metadata = fs::symlink_metadata(&current)
                .map_err(|_| internal("crate_release_owned_homes_invalid"))?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(internal("crate_release_owned_homes_invalid"));
            }
        }
        if path
            .canonicalize()
            .map_err(|_| internal("crate_release_owned_homes_invalid"))?
            != path
        {
            return Err(internal("crate_release_owned_homes_invalid"));
        }
    }
    if [rustup_home, cargo_home]
        .iter()
        .any(|home| source.starts_with(home) || home.starts_with(source))
    {
        return Err(internal("crate_release_toolchain_source_overlap"));
    }
    Ok(())
}

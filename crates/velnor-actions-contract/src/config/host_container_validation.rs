//! Admission for explicitly qualified installed container profiles.
use super::{CheckExecutor, CheckPlatform};
use super::{HostContainerProfile, HostOrbStackSdk};
use crate::errors::ContractError;

pub(super) fn validate(
    profile: &HostContainerProfile,
    platform: CheckPlatform,
    executor: CheckExecutor,
    file: &str,
    key: &str,
) -> Result<(), ContractError> {
    let bad =
        |field: &str, problem: &str| ContractError::config(file, format!("{key}.{field}"), problem);
    if !identifier(profile.context(), 128) {
        return Err(bad("context", "invalid_container_context"));
    }
    if !absolute_path(profile.socket_path()) {
        return Err(bad("socket_path", "invalid_container_socket"));
    }
    let cli = profile.cli();
    if !absolute_path(&cli.path)
        || !sha(&cli.sha256, 64)
        || !version(&cli.version)
        || !identifier(&cli.build, 128)
    {
        return Err(bad("cli", "invalid_docker_cli_pin"));
    }
    let daemon = profile.daemon();
    if !version(&daemon.version) || !printable(&daemon.operating_system, 256) {
        return Err(bad("daemon", "invalid_docker_daemon_pin"));
    }
    if platform != CheckPlatform::LinuxX64 && executor != CheckExecutor::EphemeralSelfHosted {
        return Err(bad("provider", "macos_container_requires_ephemeral_runner"));
    }
    if let HostContainerProfile::OrbStack { sdk, .. } = profile {
        if platform == CheckPlatform::LinuxX64 || executor != CheckExecutor::EphemeralSelfHosted {
            return Err(bad("provider", "invalid_orbstack_placement"));
        }
        if daemon.operating_system != "OrbStack" {
            return Err(bad(
                "daemon.operating_system",
                "orbstack_engine_identity_required",
            ));
        }
        validate_sdk(sdk).map_err(|problem| bad("sdk", problem))?;
        if std::path::Path::new(profile.socket_path()).parent()
            != Some(std::path::Path::new(&sdk.runtime_dir))
        {
            return Err(bad(
                "socket_path",
                "orbstack_socket_outside_runtime_directory",
            ));
        }
    }
    Ok(())
}

fn validate_sdk(sdk: &HostOrbStackSdk) -> Result<(), &'static str> {
    if !absolute_path(&sdk.app_bundle_path)
        || !app_extension(&sdk.app_bundle_path)
        || !absolute_path(&sdk.cli_bundle_path)
        || !app_extension(&sdk.cli_bundle_path)
        || !sdk
            .cli_bundle_path
            .starts_with(&format!("{}/Contents/", sdk.app_bundle_path))
        || !relative_path(&sdk.main_executable_path)
        || !sdk.main_executable_path.starts_with("Contents/MacOS/")
        || !relative_path(&sdk.cli_relative_path)
        || !sdk.cli_relative_path.starts_with("Contents/MacOS/")
    {
        return Err("invalid_orbstack_bundle_paths");
    }
    if !identifier(&sdk.bundle_id, 128)
        || !sdk.bundle_id.contains('.')
        || sdk.team_id.len() != 10
        || !sdk
            .team_id
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        || !version(&sdk.version)
        || !numeric(&sdk.build)
        || !version(&sdk.cli_version)
        || !numeric(&sdk.cli_build)
        || !sha(&sdk.cli_commit, 40)
    {
        return Err("invalid_orbstack_release_identity");
    }
    for digest in [
        &sdk.info_plist_sha256,
        &sdk.main_executable_sha256,
        &sdk.source_tree_sha256,
        &sdk.owned_tree_sha256,
        &sdk.cli_sha256,
    ] {
        if !sha(digest, 64) {
            return Err("invalid_orbstack_digest");
        }
    }
    if !absolute_path(&sdk.runtime_dir)
        || !sdk.runtime_dir.ends_with("/.orbstack/run")
        || sdk.runtime_uid == 0
    {
        return Err("invalid_orbstack_runtime_authority");
    }
    Ok(())
}

fn sha(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
}
fn numeric(value: &str) -> bool {
    !value.is_empty() && value.len() <= 32 && value.bytes().all(|b| b.is_ascii_digit())
}
fn version(value: &str) -> bool {
    value.len() <= 64 && value.split('.').all(numeric)
}
fn identifier(value: &str, bound: usize) -> bool {
    !value.is_empty()
        && value.len() <= bound
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        && value.as_bytes()[0].is_ascii_alphanumeric()
}
fn printable(value: &str, bound: usize) -> bool {
    !value.is_empty()
        && value.len() <= bound
        && value.trim() == value
        && value.bytes().all(|b| (b' '..=b'~').contains(&b))
        && !value.contains("${")
        && !value.contains("{{")
}
fn relative_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 1024
        && value.split('/').all(|part| {
            !part.is_empty()
                && part.len() <= 255
                && part != "."
                && part != ".."
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        })
}
fn absolute_path(value: &str) -> bool {
    value.len() <= super::super::MAX_CHECK_CONTAINER_PATH_BYTES
        && value.strip_prefix('/').is_some_and(relative_path)
}

fn app_extension(value: &str) -> bool {
    std::path::Path::new(value)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
}

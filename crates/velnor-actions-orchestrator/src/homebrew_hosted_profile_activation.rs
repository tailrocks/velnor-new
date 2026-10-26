// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    BREW_DESCRIBE, BREW_PINNED_COMMIT, BREW_PINNED_TREE, BREW_TAG_COMMIT, HOMEBREW_EXECUTABLE,
    HOMEBREW_PREFIX, HOMEBREW_VERSION, HostedHomebrewProfile, PORTABLE_RUBY_ARCHIVE_SHA256,
    PORTABLE_RUBY_VERSION, RUBY_BINARY_LIMIT,
    profile_fs::{
        PrefixIdentity, directory_identity, move_prefix, path_exists, resolve_path_executable,
        validate_no_brew_env_files, validate_trusted_host_tools,
    },
    support::{
        io_error, output_text, parse_brew_ruby_config, parse_portable_ruby_archive_sha256,
        read_small_text, require_owned_directory, run_command, sha256_file,
        validate_profile_evidence,
    },
};
use crate::{OrchestratorError, homebrew::command::HomebrewCommand, internal::internal};
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};
use velnor_actions_mise::{CheckDeadline, git_tap_capture::materialize_homebrew_pinned_source};

pub(super) fn prepare_within(
    deadline: CheckDeadline,
) -> Result<HostedHomebrewProfile, OrchestratorError> {
    let prefix = PathBuf::from(HOMEBREW_PREFIX);
    let runner_temp = std::env::var_os("RUNNER_TEMP")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| internal("homebrew_profile_runner_temp"))?;
    require_owned_directory(&runner_temp)?;
    validate_host_execution(&prefix)?;
    let old_prefix_identity = directory_identity(&prefix)?;
    let runner_metadata =
        fs::symlink_metadata(&runner_temp).map_err(|error| io_error(&runner_temp, &error))?;
    if runner_metadata.dev() != old_prefix_identity.device {
        return Err(internal("homebrew_profile_runner_device"));
    }

    let stage = tempfile::Builder::new()
        .prefix("velnor-homebrew-stage-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(&runner_temp)
        .map_err(|error| {
            OrchestratorError::io(runner_temp.display().to_string(), error.to_string())
        })?;
    let stage_path = stage.keep();
    let source = materialize_homebrew_pinned_source(&stage_path, deadline)
        .map_err(|_| internal("homebrew_profile_source"))?;
    if source.commit() != BREW_PINNED_COMMIT
        || source.tree() != BREW_PINNED_TREE
        || source.describe() != BREW_DESCRIBE
        || source.tag_commit() != BREW_TAG_COMMIT
    {
        return Err(internal("homebrew_profile_source_identity"));
    }

    let recovery = tempfile::Builder::new()
        .prefix("velnor-homebrew-recovery-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(&runner_temp)
        .map_err(|error| {
            OrchestratorError::io(runner_temp.display().to_string(), error.to_string())
        })?;
    let recovery_path = recovery.keep();
    let previous_prefix = recovery_path.join("previous");
    let stage_identity = directory_identity(&stage_path)?;
    if stage_identity.device != old_prefix_identity.device {
        return Err(internal("homebrew_profile_stage_device"));
    }
    let home = recovery_path.join("home");
    let temp = recovery_path.join("tmp");
    fs::create_dir(&home).map_err(|error| io_error(&home, &error))?;
    fs::create_dir(&temp).map_err(|error| io_error(&temp, &error))?;
    for directory in [&home, &temp] {
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
            .map_err(|error| io_error(directory, &error))?;
    }
    if move_prefix(&prefix, &previous_prefix, &home, &temp, deadline).is_err() {
        return Err(internal("homebrew_profile_old_prefix_move"));
    }
    if directory_identity(&previous_prefix)? != old_prefix_identity || path_exists(&prefix) {
        return Err(internal("homebrew_profile_old_prefix_move"));
    }
    if move_prefix(&stage_path, &prefix, &home, &temp, deadline).is_err() {
        return Err(internal("homebrew_profile_fresh_prefix_move"));
    }
    if directory_identity(&prefix)? != stage_identity || path_exists(&stage_path) {
        return Err(internal("homebrew_profile_fresh_prefix_identity"));
    }
    initialize_profile(
        &prefix,
        &previous_prefix,
        old_prefix_identity,
        &source,
        &home,
        &temp,
        deadline,
    )
}

fn validate_host_execution(prefix: &Path) -> Result<(), OrchestratorError> {
    validate_trusted_host_tools(&[
        "/usr/bin/git",
        "/usr/bin/sudo",
        "/bin/mv",
        "/bin/bash",
        "/usr/bin/lockf",
        "/usr/bin/shasum",
        "/usr/bin/curl",
        "/usr/bin/tar",
        "/usr/bin/cut",
        "/usr/bin/perl",
        "/bin/mkdir",
        "/bin/rm",
    ])?;
    validate_no_brew_env_files(prefix)?;
    let sdk_executable = std::env::current_exe()
        .map_err(|error| OrchestratorError::io("current_exe", error.to_string()))?;
    if sdk_executable
        .canonicalize()
        .map_err(|error| io_error(&sdk_executable, &error))?
        .starts_with(prefix)
    {
        return Err(internal("homebrew_profile_sdk_under_prefix"));
    }
    let mise_executable =
        resolve_path_executable("mise")?.ok_or_else(|| internal("homebrew_profile_mise_path"))?;
    if mise_executable.starts_with(prefix) {
        return Err(internal("homebrew_profile_mise_under_prefix"));
    }
    Ok(())
}

fn initialize_profile(
    prefix: &Path,
    previous_prefix: &Path,
    old_identity: PrefixIdentity,
    source: &velnor_actions_mise::git_tap_capture::PinnedHomebrewSource,
    home: &Path,
    temp: &Path,
    deadline: CheckDeadline,
) -> Result<HostedHomebrewProfile, OrchestratorError> {
    validate_no_brew_env_files(prefix)?;
    validate_trusted_host_tools(&[
        "/usr/bin/git",
        "/usr/bin/sudo",
        "/bin/mv",
        "/bin/bash",
        "/usr/bin/lockf",
        "/usr/bin/shasum",
        "/usr/bin/curl",
        "/usr/bin/tar",
        "/usr/bin/cut",
        "/usr/bin/perl",
        "/bin/mkdir",
        "/bin/rm",
    ])?;

    install_pinned_ruby(prefix, home, temp, deadline)?;
    create_fresh_install_directories(prefix)?;
    let (ruby_path, ruby_sha) = verify_portable_ruby(prefix, home, temp, deadline)?;
    let prefix_text = output_text(
        HomebrewCommand::Prefix,
        home,
        temp,
        deadline,
        HOMEBREW_EXECUTABLE,
    )?;
    if Path::new(prefix_text.trim()) != prefix {
        return Err(internal("homebrew_profile_prefix"));
    }
    let active_prefix_identity = directory_identity(prefix)?;
    let profile = HostedHomebrewProfile {
        prefix: prefix.to_path_buf(),
        previous_prefix: previous_prefix.to_path_buf(),
        previous_prefix_device: old_identity.device,
        previous_prefix_inode: old_identity.inode,
        active_prefix_device: active_prefix_identity.device,
        active_prefix_inode: active_prefix_identity.inode,
        source_commit: source.commit().to_owned(),
        source_tree: source.tree().to_owned(),
        source_describe: source.describe().to_owned(),
        source_tag_commit: source.tag_commit().to_owned(),
        portable_ruby_version: PORTABLE_RUBY_VERSION.to_owned(),
        portable_ruby_path: ruby_path,
        portable_ruby_archive_sha256: PORTABLE_RUBY_ARCHIVE_SHA256.to_owned(),
        portable_ruby_binary_sha256: ruby_sha,
    };
    validate_profile_evidence(&profile).map_err(|()| internal("homebrew_profile_evidence"))?;
    Ok(profile)
}

fn install_pinned_ruby(
    prefix: &Path,
    home: &Path,
    temp: &Path,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let version = read_small_text(
        &prefix.join("Library/Homebrew/vendor/portable-ruby-version"),
        128,
    )?;
    if version.trim() != PORTABLE_RUBY_VERSION {
        return Err(internal("homebrew_profile_ruby_source_version"));
    }
    let archive = read_small_text(
        &prefix.join("Library/Homebrew/vendor/portable-ruby-arm64-darwin"),
        4096,
    )?;
    if parse_portable_ruby_archive_sha256(&archive) != Some(PORTABLE_RUBY_ARCHIVE_SHA256) {
        return Err(internal("homebrew_profile_ruby_archive_record"));
    }
    run_command(
        HomebrewCommand::InstallPortableRuby,
        home,
        temp,
        deadline,
        "brew vendor-install ruby",
    )?;
    Ok(())
}

fn verify_portable_ruby(
    prefix: &Path,
    home: &Path,
    temp: &Path,
    deadline: CheckDeadline,
) -> Result<(PathBuf, String), OrchestratorError> {
    let configured_path = verify_brew_runtime_config(home, temp, deadline)?;
    let ruby = resolve_portable_ruby(prefix, &configured_path)?;
    let output = output_text(
        HomebrewCommand::InstalledBinaryVersion {
            executable: ruby.clone(),
        },
        home,
        temp,
        deadline,
        "portable-ruby",
    )?;
    if !output
        .lines()
        .next()
        .is_some_and(|line| line.starts_with("ruby 4.0.9 "))
    {
        return Err(internal("homebrew_profile_ruby_runtime_version"));
    }
    let hash = sha256_file(&ruby, RUBY_BINARY_LIMIT)?;
    Ok((configured_path, hash))
}

fn verify_brew_runtime_config(
    home: &Path,
    temp: &Path,
    deadline: CheckDeadline,
) -> Result<PathBuf, OrchestratorError> {
    let version = output_text(
        HomebrewCommand::Version,
        home,
        temp,
        deadline,
        HOMEBREW_EXECUTABLE,
    )?;
    if version.lines().next() != Some(HOMEBREW_VERSION) {
        return Err(internal("homebrew_profile_version"));
    }
    let config = output_text(
        HomebrewCommand::Config,
        home,
        temp,
        deadline,
        HOMEBREW_EXECUTABLE,
    )?;
    let (ruby_version, ruby_path) =
        parse_brew_ruby_config(&config).ok_or_else(|| internal("homebrew_profile_ruby_config"))?;
    if ruby_version != PORTABLE_RUBY_VERSION {
        return Err(internal("homebrew_profile_ruby_config_version"));
    }
    Ok(ruby_path)
}

fn resolve_portable_ruby(
    prefix: &Path,
    configured_path: &Path,
) -> Result<PathBuf, OrchestratorError> {
    let link = prefix.join("Library/Homebrew/vendor/portable-ruby/current");
    if fs::read_link(&link).ok().as_deref() != Some(Path::new(PORTABLE_RUBY_VERSION)) {
        return Err(internal("homebrew_profile_ruby_link"));
    }
    let version_dir = prefix
        .join("Library/Homebrew/vendor/portable-ruby")
        .join(PORTABLE_RUBY_VERSION);
    let ruby_expected = version_dir
        .join("bin/ruby")
        .canonicalize()
        .map_err(|error| io_error(&link, &error))?;
    validate_ruby_directories(&version_dir)?;
    let ruby_current = link
        .join("bin/ruby")
        .canonicalize()
        .map_err(|error| io_error(&link, &error))?;
    let configured = configured_path
        .canonicalize()
        .map_err(|error| io_error(configured_path, &error))?;
    let path_known =
        configured_path == link.join("bin/ruby") || configured_path == version_dir.join("bin/ruby");
    if ruby_current != ruby_expected || configured != ruby_expected || !path_known {
        return Err(internal("homebrew_profile_ruby_path"));
    }
    let bundle = link.join("bin/bundle");
    let metadata = fs::symlink_metadata(&bundle).map_err(|error| io_error(&bundle, &error))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(internal("homebrew_profile_ruby_bundle"));
    }
    Ok(ruby_current)
}

fn validate_ruby_directories(version_dir: &Path) -> Result<(), OrchestratorError> {
    let bin = version_dir.join("bin");
    for directory in [version_dir, bin.as_path()] {
        let metadata =
            fs::symlink_metadata(directory).map_err(|error| io_error(directory, &error))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(internal("homebrew_profile_ruby_directory"));
        }
    }
    Ok(())
}

pub(super) fn create_fresh_install_directories(prefix: &Path) -> Result<(), OrchestratorError> {
    for name in ["Cellar", "opt"] {
        let path = prefix.join(name);
        match fs::create_dir(&path) {
            Ok(()) => fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
                .map_err(|error| io_error(&path, &error))?,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(io_error(&path, &error)),
        }
        let metadata = fs::symlink_metadata(&path).map_err(|error| io_error(&path, &error))?;
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || metadata.uid() != crate::filesystem_owner::effective_uid()
            || metadata.permissions().mode() & 0o777 != 0o755
        {
            return Err(internal("homebrew_profile_install_directory"));
        }
    }
    Ok(())
}

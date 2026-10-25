use super::{
    BREW_DESCRIBE, BREW_PINNED_COMMIT, BREW_PINNED_TREE, BREW_TAG_COMMIT, CAPTURE_LIMIT,
    COMMAND_LIMIT, HOMEBREW_PREFIX, HostedHomebrewProfile, PORTABLE_RUBY_ARCHIVE_SHA256,
    PORTABLE_RUBY_VERSION, VerifiedReleaseQualificationCandidate,
};
use crate::{
    OrchestratorError,
    homebrew::command::{HomebrewCommand, homebrew_command},
    internal::internal,
};
use sha2::{Digest, Sha256};
use std::os::unix::fs::MetadataExt;
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    time::Instant,
};
use velnor_actions_mise::CheckDeadline;

pub(crate) fn verify_release_qualification_candidate()
-> Result<VerifiedReleaseQualificationCandidate, OrchestratorError> {
    let path = Path::new("manifest-assets/release-manifest.json");
    let bytes = read_regular_bounded(path, 8 * 1024 * 1024)?;
    let manifest_sha256 = std::env::var("VELNOR_RELEASE_MANIFEST_SHA256")
        .map_err(|_| internal("homebrew_profile_manifest_digest_missing"))?;
    if sha256(&bytes) != manifest_sha256 {
        return Err(internal("homebrew_profile_manifest_digest"));
    }
    let manifest = velnor_actions_contract::ReleaseManifest::parse_json_with_limit(
        &String::from_utf8(bytes).map_err(|_| internal("homebrew_profile_manifest_utf8"))?,
        "release-manifest.json",
        8 * 1024 * 1024,
    )
    .map_err(crate::internal::internal_contract)?;
    manifest
        .validate("release-manifest.json")
        .map_err(crate::internal::internal_contract)?;
    let source_sha =
        std::env::var("GITHUB_SHA").map_err(|_| internal("homebrew_profile_source_sha"))?;
    if manifest.repository != "tailrocks/velnor-new" || manifest.commit != source_sha {
        return Err(internal("homebrew_profile_manifest_identity"));
    }
    let target = manifest
        .targets
        .iter()
        .find(|target| target.target == "aarch64-apple-darwin")
        .ok_or_else(|| internal("homebrew_profile_target_missing"))?;
    let executable = std::env::current_exe()
        .map_err(|error| OrchestratorError::io("current_exe", error.to_string()))?;
    let bytes = read_regular_bounded(&executable, 512 * 1024 * 1024)?;
    if sha256(&bytes) != target.sha256 {
        return Err(internal("homebrew_profile_candidate_binary"));
    }
    Ok(VerifiedReleaseQualificationCandidate {
        manifest_sha256,
        target_sha256: target.sha256.clone(),
    })
}

pub(super) fn validate_host_identity(
    get: &dyn Fn(&str) -> Result<String, std::env::VarError>,
) -> Result<(), ()> {
    validate_host_identity_values(
        &get("GITHUB_ACTIONS").unwrap_or_default(),
        &get("RUNNER_ENVIRONMENT").unwrap_or_default(),
        &get("RUNNER_OS").unwrap_or_default(),
        &get("RUNNER_ARCH").unwrap_or_default(),
        std::env::consts::ARCH,
    )
}

pub(super) fn validate_host_identity_values(
    actions: &str,
    runner_environment: &str,
    runner_os: &str,
    runner_arch: &str,
    process_arch: &str,
) -> Result<(), ()> {
    if actions == "true"
        && runner_environment == "github-hosted"
        && runner_os == "macOS"
        && runner_arch == "ARM64"
        && process_arch == "aarch64"
    {
        Ok(())
    } else {
        Err(())
    }
}

pub(super) fn validate_profile_evidence(profile: &HostedHomebrewProfile) -> Result<(), ()> {
    if profile.prefix == Path::new(HOMEBREW_PREFIX)
        && profile.source_commit == BREW_PINNED_COMMIT
        && profile.source_tree == BREW_PINNED_TREE
        && profile.source_describe == BREW_DESCRIBE
        && profile.source_tag_commit == BREW_TAG_COMMIT
        && profile.portable_ruby_version == PORTABLE_RUBY_VERSION
        && (profile.portable_ruby_path
            == Path::new(HOMEBREW_PREFIX)
                .join("Library/Homebrew/vendor/portable-ruby/current/bin/ruby")
            || profile.portable_ruby_path
                == Path::new(HOMEBREW_PREFIX)
                    .join("Library/Homebrew/vendor/portable-ruby/4.0.9/bin/ruby"))
        && profile.portable_ruby_archive_sha256 == PORTABLE_RUBY_ARCHIVE_SHA256
        && profile.previous_prefix_device == profile.active_prefix_device
        && profile.previous_prefix_inode != profile.active_prefix_inode
        && profile
            .previous_prefix
            .file_name()
            .and_then(|name| name.to_str())
            == Some("previous")
        && profile
            .previous_prefix
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("velnor-homebrew-recovery-"))
        && is_sha256(&profile.portable_ruby_binary_sha256)
    {
        Ok(())
    } else {
        Err(())
    }
}

pub(super) fn parse_brew_ruby_config(output: &str) -> Option<(String, PathBuf)> {
    let mut configured = None;
    for line in output.lines().map(str::trim) {
        let Some(value) = line.strip_prefix("Homebrew Ruby:") else {
            continue;
        };
        if configured.is_some() {
            return None;
        }
        let (version, path) = value.trim().split_once(" => ")?;
        if version.is_empty() || path.is_empty() {
            return None;
        }
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            return None;
        }
        configured = Some((version.to_owned(), path));
    }
    configured
}

pub(super) fn parse_portable_ruby_archive_sha256(record: &str) -> Option<&str> {
    let mut sha = None;
    for line in record.lines().map(str::trim) {
        let Some(value) = line.strip_prefix("ruby_SHA=") else {
            continue;
        };
        if sha.is_some() || !is_sha256(value) {
            return None;
        }
        sha = Some(value);
    }
    sha
}

pub(super) fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn is_b3_digest(value: &str) -> bool {
    value.strip_prefix("b3-").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

pub(super) fn is_git_sha1(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn require_owned_directory(path: &Path) -> Result<(), OrchestratorError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| io_error(path, &error))?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != crate::filesystem_owner::effective_uid()
    {
        return Err(internal("homebrew_profile_directory_owner"));
    }
    Ok(())
}

pub(super) fn output_text(
    operation: HomebrewCommand,
    home: &Path,
    temp: &Path,
    deadline: CheckDeadline,
    program: &str,
) -> Result<String, OrchestratorError> {
    let output = run_command(operation, home, temp, deadline, program)?;
    String::from_utf8(output.stdout).map_err(|_| internal("homebrew_profile_output_utf8"))
}

pub(super) fn run_command(
    operation: HomebrewCommand,
    home: &Path,
    temp: &Path,
    deadline: CheckDeadline,
    program: &str,
) -> Result<velnor_actions_mise::ProcessOutput, OrchestratorError> {
    let command_deadline = CheckDeadline::from_start(Instant::now(), COMMAND_LIMIT)
        .map_err(|_| internal("homebrew_profile_command_deadline"))?;
    let command = homebrew_command(operation, home, temp)
        .map_err(|_| internal("homebrew_profile_command_rejected"))?;
    let output = deadline
        .scope(|| {
            command_deadline.scope(|| command.run_quiescent_until(CAPTURE_LIMIT, command_deadline))
        })
        .map_err(|_| internal("homebrew_profile_deadline"))?
        .map_err(|_| internal("homebrew_profile_command_failed"))?
        .map_err(|_| internal("homebrew_profile_command_failed"))?;
    let (output, _drained) = output;
    output
        .require_success(program)
        .map_err(|_| internal("homebrew_profile_command_failed"))?;
    deadline
        .remaining()
        .map_err(|_| internal("homebrew_profile_deadline"))?;
    Ok(output)
}

pub(super) fn read_small_text(path: &Path, limit: u64) -> Result<String, OrchestratorError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| io_error(path, &error))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > limit
    {
        return Err(internal("homebrew_profile_file_identity"));
    }
    let bytes = read_bounded(path, metadata.len(), limit)?;
    String::from_utf8(bytes).map_err(|_| internal("homebrew_profile_file_utf8"))
}

pub(super) fn sha256_file(path: &Path, limit: u64) -> Result<String, OrchestratorError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| io_error(path, &error))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > limit
    {
        return Err(internal("homebrew_profile_ruby_file_identity"));
    }
    Ok(sha256(&read_bounded(path, metadata.len(), limit)?))
}

pub(super) fn read_bounded(
    path: &Path,
    expected: u64,
    limit: u64,
) -> Result<Vec<u8>, OrchestratorError> {
    let mut file = File::open(path).map_err(|error| io_error(path, &error))?;
    let capacity =
        usize::try_from(expected).map_err(|_| internal("homebrew_profile_file_size_limit"))?;
    let mut bytes = Vec::with_capacity(capacity);
    file.by_ref()
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| io_error(path, &error))?;
    let actual_size =
        u64::try_from(bytes.len()).map_err(|_| internal("homebrew_profile_file_size_changed"))?;
    if actual_size != expected || actual_size > limit {
        return Err(internal("homebrew_profile_file_size_changed"));
    }
    Ok(bytes)
}

pub(super) fn read_regular_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, OrchestratorError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| io_error(path, &error))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > limit
    {
        return Err(internal("homebrew_profile_file_identity"));
    }
    read_bounded(path, metadata.len(), limit)
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut digest = Sha256::new();
    digest.update(bytes);
    let digest = digest.finalize();
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

pub(super) fn io_error(path: &Path, error: &std::io::Error) -> OrchestratorError {
    OrchestratorError::io(path.display().to_string(), error.to_string())
}

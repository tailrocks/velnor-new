//! Host admission for the Linux runner's `AppArmor` boundary.
//!
//! A Docker `security_opt` requests a profile by name. It does not prove that
//! the loaded profile is the reviewed policy or that it is in enforce mode.
//! Profiled runner starts therefore require this module's private admission
//! token before they perform any Docker or journal side effect.

use std::{fs, path::Path};

use velnor_runner_journal::HostError;

const PROFILE_NAMES: [&str; 1] = ["velnor-runner"];
const PROFILE_LIST: &str = "/sys/kernel/security/apparmor/profiles";
const PROFILE_POLICY_ROOT: &str = "/sys/kernel/security/apparmor/policy/profiles";

// There is no approved compiled policy identity yet. Keep Linux profile starts
// closed until the exact policy source and compiled bytes receive independent
// review. Do not replace `None` with a value copied from an unreviewed candidate.
const APPROVED_POLICY_SHA256: Option<&str> = None;

/// Evidence that the required named policy was present, enforcing, and matched
/// the compiled identity approved by this Velnor build.
///
/// Fields and constructors stay private. A caller cannot satisfy admission by
/// passing a boolean or inventing a profile hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunnerProfileAdmission {
    _seal: (),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdmissionFailure {
    Unapproved,
    Unavailable,
    Missing,
    NotEnforcing,
    Ambiguous,
    UnknownProfile,
    MismatchedDirectory,
    MalformedHash,
    WrongHash,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProfileRecord {
    directory: String,
    name: Option<String>,
    raw_sha256: Option<String>,
}

/// Verify every live Velnor process domain against the identity approved in
/// this binary. `AppArmor`'s profile directories are numbered and mangled, so
/// match each entry by its authoritative `name` file before reading its
/// `raw_sha256`. The kernel hash is for the exact compiled policy bytes, not a
/// hash of a mutable source file. Every attached Velnor profile must share that
/// identity and be in enforce mode.
pub fn verify_runner_profile() -> Result<RunnerProfileAdmission, HostError> {
    let expected = approved_policy_sha256().map_err(|_| HostError::Config)?;
    let profiles = fs::read_to_string(PROFILE_LIST).map_err(|_| HostError::Config)?;
    let records =
        read_profile_records(Path::new(PROFILE_POLICY_ROOT)).map_err(|_| HostError::Config)?;
    verify_observed_profiles(expected, Some(&profiles), Some(&records))
        .map_err(|_| HostError::Config)
}

fn read_profile_records(root: &Path) -> Result<Vec<ProfileRecord>, AdmissionFailure> {
    let entries = fs::read_dir(root).map_err(|_| AdmissionFailure::Unavailable)?;
    let mut records = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| AdmissionFailure::Unavailable)?;
        let directory = entry
            .file_name()
            .into_string()
            .map_err(|_| AdmissionFailure::Unavailable)?;
        let file_type = entry
            .file_type()
            .map_err(|_| AdmissionFailure::Unavailable)?;
        if !file_type.is_dir() {
            if directory.starts_with("velnor-") {
                return Err(AdmissionFailure::MismatchedDirectory);
            }
            continue;
        }

        // Read every name so an unreadable entry cannot conceal a duplicate or
        // unknown Velnor profile. Other, readable host profiles are ignored.
        let raw_name = fs::read_to_string(entry.path().join("name"))
            .map_err(|_| AdmissionFailure::Unavailable)?;
        let name = raw_name.trim_end_matches('\n');
        if name.is_empty() || name.contains('\r') || name.contains('\n') {
            return Err(AdmissionFailure::Unavailable);
        }
        if name.starts_with("velnor-")
            || directory.starts_with("velnor-")
            || PROFILE_NAMES.contains(&name)
        {
            let raw_sha256 = fs::read_to_string(entry.path().join("raw_sha256")).ok();
            records.push(ProfileRecord {
                directory,
                name: Some(name.to_owned()),
                raw_sha256,
            });
        }
    }
    Ok(records)
}

fn approved_policy_sha256() -> Result<&'static str, AdmissionFailure> {
    APPROVED_POLICY_SHA256.ok_or(AdmissionFailure::Unapproved)
}

fn verify_observed_profiles(
    expected_sha256: &str,
    profiles: Option<&str>,
    records: Option<&[ProfileRecord]>,
) -> Result<RunnerProfileAdmission, AdmissionFailure> {
    let expected = parse_sha256(expected_sha256)?;
    let profiles = profiles.ok_or(AdmissionFailure::Unavailable)?;
    verify_profile_modes(profiles)?;
    let records = records.ok_or(AdmissionFailure::Unavailable)?;
    verify_profile_records(expected, records)?;

    Ok(RunnerProfileAdmission { _seal: () })
}

fn verify_profile_modes(profiles: &str) -> Result<(), AdmissionFailure> {
    let mut found = [false; PROFILE_NAMES.len()];
    for line in profiles.lines().map(str::trim) {
        if line.is_empty() {
            continue;
        }
        let Some((name, mode)) = line
            .strip_suffix(')')
            .and_then(|entry| entry.rsplit_once(" ("))
        else {
            if line.starts_with("velnor-") {
                return Err(AdmissionFailure::UnknownProfile);
            }
            continue;
        };
        if let Some(index) = PROFILE_NAMES.iter().position(|expected| *expected == name) {
            if found[index] {
                return Err(AdmissionFailure::Ambiguous);
            }
            found[index] = true;
            if mode != "enforce" {
                return Err(AdmissionFailure::NotEnforcing);
            }
        } else if name.starts_with("velnor-") {
            // A child profile or an unknown Velnor policy can change which
            // domain an exec transition selects. Do not accept a partial or
            // differently named policy set.
            return Err(AdmissionFailure::UnknownProfile);
        }
    }

    if found.iter().any(|present| !present) {
        return Err(AdmissionFailure::Missing);
    }
    Ok(())
}

fn verify_profile_records(
    expected_sha256: &str,
    observed: &[ProfileRecord],
) -> Result<(), AdmissionFailure> {
    let expected = parse_sha256(expected_sha256)?;
    let mut found = [false; PROFILE_NAMES.len()];
    for record in observed {
        let directory_is_velnor = record.directory.starts_with("velnor-");
        let Some(name) = record.name.as_deref() else {
            return Err(AdmissionFailure::Unavailable);
        };
        let Some(index) = PROFILE_NAMES.iter().position(|expected| *expected == name) else {
            if name.starts_with("velnor-") || directory_is_velnor {
                return Err(AdmissionFailure::UnknownProfile);
            }
            continue;
        };
        if !numbered_profile_directory(&record.directory, name) {
            return Err(AdmissionFailure::MismatchedDirectory);
        }
        if found[index] {
            return Err(AdmissionFailure::Ambiguous);
        }
        found[index] = true;
        let hash = record
            .raw_sha256
            .as_deref()
            .ok_or(AdmissionFailure::Unavailable)?;
        if parse_sha256(hash.trim())? != expected {
            return Err(AdmissionFailure::WrongHash);
        }
    }
    if found.iter().any(|present| !present) {
        return Err(AdmissionFailure::Missing);
    }
    Ok(())
}

fn numbered_profile_directory(directory: &str, profile: &str) -> bool {
    let Some(suffix) = directory
        .strip_prefix(profile)
        .and_then(|remainder| remainder.strip_prefix('.'))
    else {
        return false;
    };
    !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
}

fn parse_sha256(value: &str) -> Result<&str, AdmissionFailure> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(AdmissionFailure::MalformedHash);
    }
    Ok(value)
}

/// Forge an admission token for tests. Never available in production builds.
#[cfg(any(test, feature = "test-support"))]
pub fn test_runner_profile_admission() -> RunnerProfileAdmission {
    RunnerProfileAdmission { _seal: () }
}

#[cfg(test)]
mod tests;

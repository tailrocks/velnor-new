//! Wrapper acceptance semantics; the caller supplies safely-read file evidence.
use super::invalid;
use std::collections::BTreeMap;
use velnor_actions_contract::ContractError;

/// Compiled tool authority; repository configuration cannot supply these pins.
#[derive(Debug)]
pub struct WrapperAuthority<'a> {
    /// Gradle engine distribution version.
    pub engine_version: &'a str,
    /// Reviewed generated wrapper script hash.
    pub script_sha256: &'a str,
    /// Reviewed upstream bootstrap JAR hash.
    pub jar_sha256: &'a str,
    /// Official binary distribution hash.
    pub distribution_sha256: &'a str,
}

/// Facts from one bounded, regular-file, symlink-safe evidence read.
#[derive(Debug)]
pub struct WrapperEvidence<'a> {
    /// Hash of safely-read script bytes.
    pub script_sha256: &'a str,
    /// Executable mode from the same script handle.
    pub script_executable: bool,
    /// Hash of safely-read JAR bytes.
    pub jar_sha256: &'a str,
    /// Daemon criteria can override the qualified Java installation.
    pub daemon_criteria_present: bool,
    /// Safely-read wrapper properties content.
    pub properties: &'a str,
}

/// Exact bootstrap bytes and distribution properties must match the authority.
/// # Errors
/// Rejects invalid pins, mismatched bytes, nonexecutable scripts, or altered properties.
pub fn validate_wrapper(
    authority: &WrapperAuthority<'_>,
    evidence: &WrapperEvidence<'_>,
) -> Result<(), ContractError> {
    if evidence.daemon_criteria_present {
        return Err(invalid("gradle_daemon_jvm_criteria_unsupported"));
    }
    if ![
        authority.script_sha256,
        authority.jar_sha256,
        authority.distribution_sha256,
    ]
    .iter()
    .all(|value| valid_sha(value))
    {
        return Err(invalid("gradle_wrapper_authority_invalid"));
    }
    if !evidence.script_executable {
        return Err(invalid("gradle_wrapper_script_not_executable"));
    }
    if authority.script_sha256 != evidence.script_sha256
        || authority.jar_sha256 != evidence.jar_sha256
    {
        return Err(invalid("gradle_wrapper_bytes_mismatch"));
    }
    validate_properties(evidence.properties, authority)
}

fn valid_sha(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && value.bytes().any(|byte| byte != b'0')
}

fn validate_properties(text: &str, authority: &WrapperAuthority<'_>) -> Result<(), ContractError> {
    let values = properties(text)?;
    let distribution = format!(
        "https\\://services.gradle.org/distributions/gradle-{version}-bin.zip",
        version = authority.engine_version
    );
    let expected = [
        ("distributionBase", "GRADLE_USER_HOME"),
        ("distributionPath", "wrapper/dists"),
        ("distributionUrl", distribution.as_str()),
        ("distributionSha256Sum", authority.distribution_sha256),
        ("networkTimeout", "10000"),
        ("validateDistributionUrl", "true"),
        ("zipStoreBase", "GRADLE_USER_HOME"),
        ("zipStorePath", "wrapper/dists"),
    ];
    if values.len() != expected.len()
        || expected
            .iter()
            .any(|(key, value)| values.get(*key).copied() != Some(*value))
    {
        return Err(invalid("gradle_wrapper_distribution_authority_mismatch"));
    }
    Ok(())
}

fn properties(text: &str) -> Result<BTreeMap<&str, &str>, ContractError> {
    let mut values = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim_start();
        if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(invalid("gradle_wrapper_properties_unsupported"));
        };
        if values.insert(key, value).is_some() {
            return Err(invalid("gradle_wrapper_properties_duplicate"));
        }
    }
    Ok(values)
}

#[cfg(test)]
#[path = "wrapper_tests.rs"]
mod tests;

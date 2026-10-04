//! Bind safely-read repository evidence to the native wrapper domain authority.

use super::{evidence_file, evidence_with_mode, failed, relative};
use crate::OrchestratorError;
use velnor_actions_contract::FileIndex;
use velnor_actions_native::java::{WrapperAuthority, WrapperEvidence, validate_wrapper};

pub(super) fn validate(root: &str, index: &FileIndex) -> Result<(), OrchestratorError> {
    use velnor_actions_mise::catalog::{
        GRADLE_WRAPPER_DISTRIBUTION_SHA256, GRADLE_WRAPPER_JAR_SHA256,
        GRADLE_WRAPPER_SCRIPT_SHA256, GRADLE_WRAPPER_VERSION,
    };
    let (script, mode) = evidence_with_mode(root, "gradlew", index)?;
    let jar = evidence_file(root, "gradle/wrapper/gradle-wrapper.jar", index)?;
    let properties = evidence_file(root, "gradle/wrapper/gradle-wrapper.properties", index)?;
    let properties =
        String::from_utf8(properties).map_err(|_| failed("gradle_wrapper_properties_not_utf8"))?;
    let daemon_criteria_present = match std::fs::symlink_metadata(
        index
            .root()
            .join(relative(root, "gradle/gradle-daemon-jvm.properties")),
    ) {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => return Err(failed("gradle_daemon_jvm_criteria_unreadable")),
    };
    validate_wrapper(
        &WrapperAuthority {
            engine_version: GRADLE_WRAPPER_VERSION,
            script_sha256: GRADLE_WRAPPER_SCRIPT_SHA256,
            jar_sha256: GRADLE_WRAPPER_JAR_SHA256,
            distribution_sha256: GRADLE_WRAPPER_DISTRIBUTION_SHA256,
        },
        &WrapperEvidence {
            script_sha256: &crate::cover_identity::generator::sha256_hex(&script),
            script_executable: mode & 0o111 != 0,
            daemon_criteria_present,
            jar_sha256: &crate::cover_identity::generator::sha256_hex(&jar),
            properties: &properties,
        },
    )?;
    Ok(())
}

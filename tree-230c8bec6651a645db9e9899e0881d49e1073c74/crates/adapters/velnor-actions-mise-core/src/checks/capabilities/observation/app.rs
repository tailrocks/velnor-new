//! Signed `OrbStack` app and nested CLI identity validation.

use serde::{Deserialize, Serialize};
use velnor_actions_contract_config::config::{
    HostOrbStackSdk, MAX_CHECK_CONTAINER_APP_INFO_CAPTURE_BYTES,
    MAX_CHECK_CONTAINER_APP_VERIFY_CAPTURE_BYTES, MAX_CHECK_CONTAINER_IDENTITY_CAPTURE_BYTES,
};

use crate::MiseError;

use super::{ContainerProbeOutput, invalid, validate_streams};

/// Actual signed application identity, separate from CLI reported version/build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrbStackAppObservation {
    /// Actual Info.plist bundle identifier.
    pub bundle_id: String,
    /// Actual application marketing release.
    pub version: String,
    /// Actual application bundle build.
    pub build: String,
    /// Actual signed developer team identity.
    pub team_id: String,
    /// Exact Info.plist JSON observation.
    pub info: ContainerProbeOutput,
    /// Exact codesign identity observation.
    pub signature: ContainerProbeOutput,
    /// Successful strict outer application verification.
    pub outer_integrity: ContainerProbeOutput,
    /// Successful strict source nested CLI verification.
    pub source_cli_integrity: ContainerProbeOutput,
    /// Successful strict owned nested CLI verification.
    pub owned_cli_integrity: ContainerProbeOutput,
    /// Actual source nested CLI signed identity.
    pub source_cli_signature: ContainerProbeOutput,
    /// Actual owned nested CLI signed identity.
    pub owned_cli_signature: ContainerProbeOutput,
}

impl OrbStackAppObservation {
    pub(crate) fn parse(
        info: ContainerProbeOutput,
        signature: ContainerProbeOutput,
        outer_integrity: ContainerProbeOutput,
        source_cli_integrity: ContainerProbeOutput,
        owned_cli_integrity: ContainerProbeOutput,
        source_cli_signature: ContainerProbeOutput,
        owned_cli_signature: ContainerProbeOutput,
    ) -> Result<Self, MiseError> {
        validate_streams(&info, MAX_CHECK_CONTAINER_APP_INFO_CAPTURE_BYTES)?;
        for output in [&signature, &source_cli_signature, &owned_cli_signature] {
            validate_streams(output, MAX_CHECK_CONTAINER_IDENTITY_CAPTURE_BYTES)?;
        }
        for output in [
            &outer_integrity,
            &source_cli_integrity,
            &owned_cli_integrity,
        ] {
            validate_streams(output, MAX_CHECK_CONTAINER_APP_VERIFY_CAPTURE_BYTES)?;
        }
        let json: serde_json::Value = serde_json::from_str(&info.stdout)
            .map_err(|_| invalid("orbstack_app", "invalid_plist_json"))?;
        let value = |key: &str| {
            json.get(key)
                .and_then(serde_json::Value::as_str)
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| invalid("orbstack_app", "missing_identity_field"))
        };
        let bundle_id = value("CFBundleIdentifier")?;
        let version = value("CFBundleShortVersionString")?;
        let build = value("CFBundleVersion")?;
        let teams: Vec<_> = signature
            .stderr
            .lines()
            .filter_map(|line| line.strip_prefix("TeamIdentifier="))
            .collect();
        let ids: Vec<_> = signature
            .stderr
            .lines()
            .filter_map(|line| line.strip_prefix("Identifier="))
            .collect();
        if teams.len() != 1 || teams[0].is_empty() || ids != [bundle_id.as_str()] {
            return Err(invalid("orbstack_app", "missing_signed_identity"));
        }
        let team_id = teams[0].to_owned();
        let source_identity = signed_identity(&source_cli_signature)?;
        let owned_identity = signed_identity(&owned_cli_signature)?;
        if source_identity != owned_identity || source_identity.0 != team_id {
            return Err(invalid("orbstack_app", "nested_signed_identity_mismatch"));
        }
        Ok(Self {
            bundle_id,
            version,
            build,
            team_id,
            info,
            signature,
            outer_integrity,
            source_cli_integrity,
            owned_cli_integrity,
            source_cli_signature,
            owned_cli_signature,
        })
    }

    pub(super) fn validate(&self, sdk: &HostOrbStackSdk) -> Result<(), MiseError> {
        validate_main_executable(&self.info.stdout, &sdk.main_executable_path)?;
        let parsed = Self::parse(
            self.info.clone(),
            self.signature.clone(),
            self.outer_integrity.clone(),
            self.source_cli_integrity.clone(),
            self.owned_cli_integrity.clone(),
            self.source_cli_signature.clone(),
            self.owned_cli_signature.clone(),
        )?;
        if &parsed != self
            || self.bundle_id != sdk.bundle_id
            || self.version != sdk.version
            || self.build != sdk.build
            || self.team_id != sdk.team_id
        {
            return Err(invalid("orbstack_app", "declared_signed_app_mismatch"));
        }
        Ok(())
    }
}

pub(super) fn validate_main_executable(info: &str, declared: &str) -> Result<(), MiseError> {
    let json: serde_json::Value =
        serde_json::from_str(info).map_err(|_| invalid("orbstack_app", "invalid_plist_json"))?;
    let name = json
        .get("CFBundleExecutable")
        .and_then(serde_json::Value::as_str)
        .filter(|name| {
            !name.is_empty() && !name.contains(['/', '\\']) && !matches!(*name, "." | "..")
        })
        .ok_or_else(|| invalid("orbstack_app", "invalid_main_executable_name"))?;
    if declared != format!("Contents/MacOS/{name}") {
        return Err(invalid("orbstack_app", "declared_main_executable_mismatch"));
    }
    Ok(())
}

fn signed_identity(probe: &ContainerProbeOutput) -> Result<(&str, &str), MiseError> {
    let values = |prefix| {
        probe
            .stderr
            .lines()
            .filter_map(move |line| line.strip_prefix(prefix))
            .collect::<Vec<_>>()
    };
    let teams = values("TeamIdentifier=");
    let ids = values("Identifier=");
    if teams.len() != 1 || ids.len() != 1 || teams[0].is_empty() || ids[0].is_empty() {
        return Err(invalid("orbstack_app", "missing_nested_signed_identity"));
    }
    Ok((teams[0], ids[0]))
}

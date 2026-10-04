//! Explicit optional native delivery families.

use super::{AptDeliveryConfig, DesktopDeliveryConfig, OciReleaseConfig};
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

/// Native publication workflows; omitted families emit no delivery files.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryConfig {
    /// Signed Debian package and APT repository publication.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apt: Option<AptDeliveryConfig>,
    /// Native desktop build, signing, and release publication.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desktop: Option<DesktopDeliveryConfig>,
    /// Native multi-platform OCI publication.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oci: Option<OciReleaseConfig>,
}

impl DeliveryConfig {
    /// Whether any delivery family was explicitly configured.
    #[must_use]
    pub fn is_configured(&self) -> bool {
        self.apt.is_some() || self.desktop.is_some() || self.oci.is_some()
    }

    /// Validate every explicitly configured family.
    /// # Errors
    /// Returns the first family diagnostic with its configuration key path.
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if let Some(apt) = &self.apt {
            apt.validate(file)?;
        }
        if let Some(desktop) = &self.desktop {
            desktop.validate(file)?;
        }
        if let Some(oci) = &self.oci {
            oci.validate(file)?;
        }
        Ok(())
    }
}

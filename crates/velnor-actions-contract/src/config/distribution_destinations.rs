//! Closed channel-to-formula authority; credentials remain shared in the parent.
use super::{config_error, valid_formula_path};
use crate::{ContractError, package_release::PackageReleaseChannel};
use serde::{Deserialize, Serialize};

/// One literal formula path within the configured tap repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomebrewTapDestination {
    /// Exact formula path admitted by the canonical formula spec.
    pub formula: String,
}

/// Exactly the two native channels, with no arbitrary source or destination keys.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomebrewTapDestinations {
    /// Rolling-preview formula destination.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<HomebrewTapDestination>,
    /// Permanent stable-release formula destination.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stable: Option<HomebrewTapDestination>,
}

impl HomebrewTapDestinations {
    /// Select a destination only from the authenticated channel enum.
    #[must_use]
    pub const fn get(&self, channel: PackageReleaseChannel) -> Option<&HomebrewTapDestination> {
        match channel {
            PackageReleaseChannel::Preview => self.preview.as_ref(),
            PackageReleaseChannel::Stable => self.stable.as_ref(),
        }
    }

    /// Iterate only explicitly declared native channel destinations.
    pub fn iter(&self) -> impl Iterator<Item = (PackageReleaseChannel, &HomebrewTapDestination)> {
        [
            (PackageReleaseChannel::Preview, self.preview.as_ref()),
            (PackageReleaseChannel::Stable, self.stable.as_ref()),
        ]
        .into_iter()
        .filter_map(|(channel, destination)| destination.map(|value| (channel, value)))
    }

    pub(super) fn validate(
        &self,
        file: &str,
        enabled: bool,
        updater_inputs: Option<&[String]>,
    ) -> Result<(), ContractError> {
        let key = "distribution.homebrew_tap.destinations";
        if enabled && self.preview.is_none() && self.stable.is_none() {
            return Err(config_error(
                file,
                key,
                "channels",
                "at_least_one_channel_required",
            ));
        }
        for (_, destination) in self.iter() {
            if !valid_formula_path(&destination.formula) {
                return Err(config_error(file, key, "formula", "bad_formula_path"));
            }
            if updater_inputs.is_some_and(|inputs| !inputs.contains(&destination.formula)) {
                return Err(config_error(
                    file,
                    key,
                    "formula",
                    "formula_base_not_an_input",
                ));
            }
        }
        if let (Some(preview), Some(stable)) = (&self.preview, &self.stable)
            && preview.formula == stable.formula
        {
            return Err(config_error(
                file,
                key,
                "formula",
                "channel_formula_paths_must_be_distinct",
            ));
        }
        Ok(())
    }
}

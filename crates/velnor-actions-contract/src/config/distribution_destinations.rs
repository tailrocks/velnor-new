// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{config_error, valid_formula_path};
use crate::{ContractError, package_release::PackageReleaseChannel};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomebrewTapDestination {
    pub formula: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomebrewTapDestinations {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<HomebrewTapDestination>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stable: Option<HomebrewTapDestination>,
}

impl HomebrewTapDestinations {
    #[must_use]
    pub const fn get(&self, channel: PackageReleaseChannel) -> Option<&HomebrewTapDestination> {
        match channel {
            PackageReleaseChannel::Preview => self.preview.as_ref(),
            PackageReleaseChannel::Stable => self.stable.as_ref(),
        }
    }

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

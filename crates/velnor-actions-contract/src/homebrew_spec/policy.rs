// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    BTreeSet, ContractError, HOMEBREW_FORMULA_SPEC_SCHEMA_V1, HOMEBREW_FORMULA_TEMPLATE_V1,
    HomebrewAuxiliaryTest, HomebrewFormulaSpec, HomebrewInstalledMode, canonical_json_bytes,
    channel_key, formula_cpu_class, ruby_class_name, safe_archive_member, safe_binary_name,
    safe_formula_name, safe_formula_path, safe_identifier, safe_relative_path,
    sorted_unique_strings, supported_formula_target, valid_https_url, valid_iso_date,
    valid_ruby_string, valid_spdx_expression,
};

impl HomebrewFormulaSpec {
    pub fn validate(&self) -> Result<(), ContractError> {
        self.validate_metadata()?;
        self.validate_channels()?;
        self.validate_host()?;
        self.validate_auxiliaries()?;
        self.validate_destinations()?;
        self.validate_sentinel()?;
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ContractError> {
        self.validate()?;
        canonical_json_bytes(self)
    }

    pub fn digest(&self) -> Result<String, ContractError> {
        Ok(crate::canonical::digest_b3(&self.canonical_bytes()?))
    }

    fn validate_metadata(&self) -> Result<(), ContractError> {
        let bad = |problem| ContractError::identity("homebrew_formula_spec", problem);
        if self.schema != HOMEBREW_FORMULA_SPEC_SCHEMA_V1 {
            return Err(bad("unknown_schema"));
        }
        if self.template != HOMEBREW_FORMULA_TEMPLATE_V1 {
            return Err(bad("unknown_template"));
        }
        if !valid_ruby_string(&self.description, 512)
            || !self.description.is_ascii()
            || !self.homepage.is_ascii()
            || !valid_https_url(&self.homepage)
            || !valid_spdx_expression(&self.license)
        {
            return Err(bad("invalid_formula_metadata"));
        }
        Ok(())
    }

    fn validate_channels(&self) -> Result<(), ContractError> {
        let bad = |problem| ContractError::identity("homebrew_formula_spec", problem);
        if self.channels.is_empty()
            || self.channels.len() > 2
            || self
                .channels
                .windows(2)
                .any(|pair| channel_key(pair[0].channel) >= channel_key(pair[1].channel))
        {
            return Err(bad("channels_not_canonical"));
        }
        for channel in &self.channels {
            if !safe_formula_name(&channel.formula)
                || channel.formula_class != ruby_class_name(&channel.formula)
                || channel.destination != format!("Formula/{}.rb", channel.formula)
                || !safe_formula_path(&channel.destination)
                || channel.conflicts.windows(2).any(|pair| pair[0] >= pair[1])
                || channel
                    .conflicts
                    .iter()
                    .any(|value| !safe_formula_name(value))
                || channel
                    .conflicts
                    .iter()
                    .any(|value| value == &channel.formula)
            {
                return Err(bad("invalid_channel_destination_or_conflicts"));
            }
        }
        if self
            .channels
            .iter()
            .map(|channel| channel.formula.as_str())
            .collect::<BTreeSet<_>>()
            .len()
            != self.channels.len()
            || self
                .channels
                .iter()
                .map(|channel| channel.destination.as_str())
                .collect::<BTreeSet<_>>()
                .len()
                != self.channels.len()
        {
            return Err(bad("channel_formula_identity_collision"));
        }
        Ok(())
    }

    fn validate_host(&self) -> Result<(), ContractError> {
        let bad = |problem| ContractError::identity("homebrew_formula_spec", problem);
        if !safe_identifier(&self.host.product)
            || !sorted_unique_strings(&self.host.targets, supported_formula_target)
            || !sorted_unique_strings(&self.host.bins, safe_binary_name)
            || self.host.targets.is_empty()
            || self.host.bins.is_empty()
        {
            return Err(bad("invalid_host_selection"));
        }
        Ok(())
    }

    fn validate_auxiliaries(&self) -> Result<(), ContractError> {
        let bad = |problem| ContractError::identity("homebrew_formula_spec", problem);
        if self
            .auxiliaries
            .windows(2)
            .any(|pair| pair[0].product >= pair[1].product)
            || self.auxiliaries.iter().any(|auxiliary| {
                !safe_identifier(&auxiliary.product)
                    || !sorted_unique_strings(&auxiliary.targets, supported_formula_target)
                    || auxiliary.targets.is_empty()
                    || auxiliary
                        .targets
                        .iter()
                        .filter_map(|target| formula_cpu_class(target))
                        .collect::<BTreeSet<_>>()
                        .len()
                        != auxiliary.targets.len()
                    || auxiliary.files.is_empty()
                    || auxiliary.files.windows(2).any(|pair| {
                        (&pair[0].target, &pair[0].member) >= (&pair[1].target, &pair[1].member)
                    })
                    || auxiliary.files.iter().any(|file| {
                        !auxiliary.targets.contains(&file.target)
                            || !safe_archive_member(&file.member)
                            || !safe_relative_path(&file.destination)
                            || (file.mode == HomebrewInstalledMode::Data
                                && file.test != HomebrewAuxiliaryTest::None)
                            || (file.mode == HomebrewInstalledMode::Executable
                                && file.test == HomebrewAuxiliaryTest::None)
                            || (file.test == HomebrewAuxiliaryTest::LinuxElf
                                && !matches!(
                                    file.target.as_str(),
                                    "aarch64-unknown-linux-gnu" | "x86_64-unknown-linux-gnu"
                                ))
                    })
            })
        {
            return Err(bad("invalid_auxiliary_selection"));
        }
        Ok(())
    }

    fn validate_destinations(&self) -> Result<(), ContractError> {
        let bad = |problem| ContractError::identity("homebrew_formula_spec", problem);
        let effective_paths = self
            .auxiliaries
            .iter()
            .flat_map(|auxiliary| &auxiliary.files)
            .map(|file| {
                (
                    formula_cpu_class(&file.target).unwrap_or(""),
                    file.destination.as_str(),
                )
            })
            .collect::<Vec<_>>();
        for (index, (cpu, path)) in effective_paths.iter().enumerate() {
            for (other_cpu, other_path) in effective_paths.iter().skip(index + 1) {
                if cpu == other_cpu
                    && (path == other_path
                        || other_path
                            .strip_prefix(path)
                            .is_some_and(|suffix| suffix.starts_with('/'))
                        || path
                            .strip_prefix(other_path)
                            .is_some_and(|suffix| suffix.starts_with('/')))
                {
                    return Err(bad("auxiliary_destination_collision"));
                }
            }
        }
        Ok(())
    }

    fn validate_sentinel(&self) -> Result<(), ContractError> {
        let bad = |problem| ContractError::identity("homebrew_formula_spec", problem);
        if !valid_iso_date(&self.stable_sentinel.date)
            || !valid_ruby_string(&self.stable_sentinel.reason, 256)
            || !self.stable_sentinel.reason.is_ascii()
        {
            return Err(bad("invalid_stable_sentinel"));
        }
        Ok(())
    }
}

use super::stacks::{RustConfiguration, is_valid_feature_name, is_valid_rust_target};
use crate::errors::ContractError;
use crate::ids::is_component_byte;
use std::collections::BTreeSet;

pub(super) fn validate_rust_configuration<'a>(
    config: &'a RustConfiguration,
    names: &mut BTreeSet<&'a str>,
    file: &str,
) -> Result<(), ContractError> {
    if config.name.trim().is_empty() {
        return Err(ContractError::config(
            file,
            "stacks.rust.configurations.name",
            "empty_name",
        ));
    }
    if !config.name.bytes().all(is_component_byte) {
        return Err(ContractError::config(
            file,
            "stacks.rust.configurations.name",
            format!("bad_component:{}", config.name),
        ));
    }
    if !names.insert(config.name.as_str()) {
        return Err(ContractError::config(
            file,
            "stacks.rust.configurations",
            format!("duplicate_configuration:{}", config.name),
        ));
    }
    if !is_valid_rust_target(&config.target) {
        return Err(ContractError::config(
            file,
            "stacks.rust.configurations.target",
            format!("bad_target:{}", config.target),
        ));
    }
    for feature in &config.features {
        if !is_valid_feature_name(feature) {
            return Err(ContractError::config(
                file,
                "stacks.rust.configurations.features",
                format!("bad_feature:{feature}"),
            ));
        }
    }
    let scoped: BTreeSet<&str> = config.packages.iter().map(String::as_str).collect();
    if scoped.len() != config.packages.len() || scoped.iter().any(|p| p.trim().is_empty()) {
        return Err(ContractError::config(
            file,
            "stacks.rust.configurations.packages",
            format!("bad_package_list:{}", config.name),
        ));
    }
    let default_features = config.features.is_empty() || config.features == ["default"];
    if config.all_features && !default_features {
        return Err(ContractError::config(
            file,
            "stacks.rust.configurations.features",
            "features_conflict_all_features",
        ));
    }
    Ok(())
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, config::CONFIG_REL, release_identity::head_sha};
use std::collections::BTreeMap;
use velnor_actions_contract::config::RustReleaseConfig;
use velnor_actions_rust::release_select::ReleaseSelection;
use velnor_actions_workflow_renderer::release_spec::{
    DispatchInput, ReleasePlan, ReleaseTriggers, validate_source_sha,
};

pub(super) fn check_creation_selection(
    release: &RustReleaseConfig,
    selection: &ReleaseSelection,
) -> Result<(), OrchestratorError> {
    if let Some(creation) = &release.new_crates {
        for (name, version) in &creation.packages {
            if !selection
                .packages
                .iter()
                .any(|package| &package.name == name && &package.version == version)
            {
                return Err(OrchestratorError::config(
                    CONFIG_REL,
                    "stacks.rust.release.new_crates.packages",
                    "creation_package_version_mismatch",
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn check_recovery_record(
    release: &RustReleaseConfig,
    selection: &ReleaseSelection,
) -> Result<(), OrchestratorError> {
    let Some(record) = release.recovery.as_ref() else {
        return Ok(());
    };
    if selection.packages.len() != 1 {
        return Err(OrchestratorError::config(
            CONFIG_REL,
            "stacks.rust.release.recovery.package",
            "recovery_requires_single_selected_package",
        ));
    }
    let selected = selection
        .packages
        .iter()
        .find(|package| package.name == record.package);
    let Some(selected) = selected else {
        return Err(OrchestratorError::config(
            CONFIG_REL,
            "stacks.rust.release.recovery.package",
            "recovery_package_not_selected",
        ));
    };
    if selected.version != record.version {
        return Err(OrchestratorError::config(
            CONFIG_REL,
            "stacks.rust.release.recovery.version",
            format!("recovery_version_mismatch:{}", selected.version),
        ));
    }
    Ok(())
}

pub(super) fn approved_source(
    root: &std::path::Path,
    release: &RustReleaseConfig,
) -> Result<String, OrchestratorError> {
    if let Some(record) = release.recovery.as_ref() {
        validate_source_sha(&record.source_sha)?;
        return Ok(record.source_sha.clone());
    }
    if let Some(creation) = &release.new_crates {
        validate_source_sha(&creation.source_sha)?;
        return Ok(creation.source_sha.clone());
    }
    head_sha(root)
}

pub(super) fn release_plan(
    selection: &ReleaseSelection,
    repository: &str,
    sha: &str,
    plan_id: &str,
    registry: &str,
    version: Option<&str>,
    dispatch: bool,
) -> ReleasePlan {
    let packages: BTreeMap<String, String> = selection
        .packages
        .iter()
        .map(|package| (package.name.clone(), package.version.clone()))
        .collect();
    ReleasePlan {
        plan_id: plan_id.to_owned(),
        repository: repository.to_owned(),
        source_sha: sha.to_owned(),
        registry: registry.to_owned(),
        packages,
        version: version.map(str::to_owned),
        dispatch,
    }
}

pub(super) fn release_triggers(branch: &str, plan: &ReleasePlan, creation: bool) -> ReleaseTriggers {
    if !plan.dispatch {
        return ReleaseTriggers {
            push_branches: vec![branch.to_owned()],
            schedule: None,
            dispatch_inputs: Vec::new(),
        };
    }
    let mut dispatch_inputs = vec![
        DispatchInput {
            name: "plan".to_owned(),
            description: "Approved release plan".to_owned(),
            required: true,
            default: Some(plan.plan_id.clone()),
        },
        DispatchInput {
            name: "source_sha".to_owned(),
            description: "Approved source SHA".to_owned(),
            required: true,
            default: Some(plan.source_sha.clone()),
        },
    ];
    if let Some(version) = &plan.version {
        dispatch_inputs.push(DispatchInput {
            name: "version".to_owned(),
            description: "Approved crate version".to_owned(),
            required: true,
            default: Some(version.clone()),
        });
    }
    ReleaseTriggers {
        push_branches: if creation { Vec::new() } else { vec![branch.to_owned()] },
        schedule: None,
        dispatch_inputs,
    }
}

pub(super) fn reviewed_generator_manifest(
    raw: Option<&str>,
    stand_in: bool,
    version: &str,
) -> Result<(String, String), OrchestratorError> {
    use sha2::{Digest, Sha256};
    if stand_in {
        return Err(OrchestratorError::unsupported(
            "generator_manifest",
            "release requires a reviewed consumer release manifest",
        ));
    }
    let raw = raw.ok_or_else(|| {
        OrchestratorError::unsupported(
            "generator_manifest",
            "release requires a reviewed consumer release manifest",
        )
    })?;
    let manifest =
        velnor_actions_workflow_renderer::release_helpers::validate_consumer_manifest_for_version(
            velnor_actions_contract::ReleaseTarget::LinuxX86_64,
            version,
            Some(raw),
        )
        .map_err(crate::local_release_manifest::map_release_helper_error)?;
    let mut digest = String::with_capacity(64);
    for byte in Sha256::digest(raw.as_bytes()) {
        use std::fmt::Write as _;
        write!(&mut digest, "{byte:02x}").map_err(|_| OrchestratorError::Contract {
            problem: "generator_manifest_digest_encoding".to_owned(),
        })?;
    }
    Ok((manifest.commit, digest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_publication_is_dispatch_only() {
        let plan = ReleasePlan {
            plan_id: "test".into(),
            repository: "owner/repo".into(),
            source_sha: "a".repeat(40),
            registry: "crates-io".into(),
            packages: BTreeMap::new(),
            version: None,
            dispatch: true,
        };
        let creation = release_triggers("main", &plan, true);
        assert!(creation.push_branches.is_empty());
        assert!(!creation.dispatch_inputs.is_empty());
        let recovery = release_triggers("main", &plan, false);
        assert_eq!(recovery.push_branches, ["main"]);
    }
}

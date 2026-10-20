use std::collections::BTreeMap;

use velnor_actions_contract::config::RustReleaseConfig;
use velnor_actions_rust::release_select::ReleaseSelection;
use velnor_actions_workflow_renderer::release_config::{ReleasePlzConfig, ReleasePlzPackage};
use velnor_actions_workflow_renderer::release_spec::{
    BootstrapPlan, DispatchInput, ReleaseTriggers,
};

/// Assemble the approved exact-source plan from selection plus identity.
///
/// `version` is the configured bootstrap version, present only in
/// bootstrap-token mode (config validation forbids the record under
/// trusted publishing); routine publishers resolve versions at runtime.
pub(super) fn bootstrap_plan(
    selection: &ReleaseSelection,
    repository: &str,
    sha: &str,
    plan_id: &str,
    registry: &str,
    version: Option<&str>,
) -> BootstrapPlan {
    let packages: BTreeMap<String, String> = selection
        .packages
        .iter()
        .map(|package| (package.name.clone(), package.version.clone()))
        .collect();
    BootstrapPlan {
        plan_id: plan_id.to_owned(),
        repository: repository.to_owned(),
        source_sha: sha.to_owned(),
        registry: registry.to_owned(),
        packages,
        version: version.map(str::to_owned),
    }
}

/// Trusted-branch push plus plan-bound dispatch (no schedule: the config
/// schema carries no cron field, so emission invents none).
pub(super) fn release_triggers(branch: &str, bootstrap: &BootstrapPlan) -> ReleaseTriggers {
    let mut dispatch_inputs = vec![
        DispatchInput {
            name: "plan".to_owned(),
            description: "Approved release plan".to_owned(),
            required: true,
            default: Some(bootstrap.plan_id.clone()),
        },
        DispatchInput {
            name: "source_sha".to_owned(),
            description: "Approved source SHA".to_owned(),
            required: true,
            default: Some(bootstrap.source_sha.clone()),
        },
    ];
    if let Some(version) = &bootstrap.version {
        dispatch_inputs.push(DispatchInput {
            name: "version".to_owned(),
            description: "Approved crate version".to_owned(),
            required: true,
            default: Some(version.clone()),
        });
    }
    ReleaseTriggers {
        push_branches: vec![branch.to_owned()],
        schedule: None,
        dispatch_inputs,
    }
}

/// Effective normal-policy config: tag plus allowlist, no features.
///
/// The schema carries no `publish_features`, so emission sets none;
/// `semver_check` stays true per the release contract.
pub(super) fn plz_config(
    release: &RustReleaseConfig,
    selection: &ReleaseSelection,
) -> ReleasePlzConfig {
    ReleasePlzConfig {
        tag_pattern: release.tag_name.clone(),
        semver_check: true,
        packages: selection
            .packages
            .iter()
            .map(|package| ReleasePlzPackage {
                name: package.name.clone(),
                publish_features: Vec::new(),
            })
            .collect(),
    }
}

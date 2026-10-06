//! Task proposal derivation, split from discovery coordination.
use velnor_actions_contract::{
    DetectionStatus, FileIndex, ProposedTask, RustStackConfig, VelnorConfig,
};
use velnor_actions_mise::ArchivePlan;
use velnor_actions_rust::propose_task;

use super::PlannedWorkspace;
use crate::OrchestratorError;

/// Derive every task proposal (rust groups plus tofu triples) and fallbacks.
pub(super) fn derive_all(
    config: &VelnorConfig,
    index: &FileIndex,
    workspaces: &[PlannedWorkspace],
    statuses: &[DetectionStatus],
) -> Result<
    (
        Vec<ProposedTask>,
        Vec<crate::derive_groups::FeatureFallback>,
    ),
    OrchestratorError,
> {
    let rust = config
        .stacks
        .rust
        .clone()
        .unwrap_or_else(RustStackConfig::default_config);
    let explicit_fmt = index.contains("rustfmt.toml") || index.contains(".rustfmt.toml");
    let union = crate::derive_groups::declared_union(workspaces, index);
    let mut groups = Vec::new();
    let mut fallbacks = Vec::new();
    let mut archives = ArchivePlan::new();
    for workspace in workspaces {
        for config_name in &rust.configurations {
            let (derived, narrowed) = crate::derive_groups::derive_for_config(
                config,
                index,
                workspace,
                config_name,
                explicit_fmt,
                &mut archives,
                &union,
            )?;
            groups.extend(derived);
            fallbacks.extend(narrowed);
        }
    }
    let mut proposals = Vec::with_capacity(groups.len());
    for group in &groups {
        let task = propose_task(group)?;
        task.validate()?;
        proposals.push(task);
    }
    proposals.extend(crate::select_tofu::derive_tofu(statuses, index.files())?);
    proposals.extend(crate::workloads::derive(config, index)?);
    proposals.sort_by(|left, right| left.task_id.cmp(&right.task_id));
    Ok((proposals, fallbacks))
}

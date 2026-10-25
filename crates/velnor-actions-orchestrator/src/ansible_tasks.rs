use velnor_actions_contract::{JobTimeout, VelnorConfig};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::verification_jobs::{StackJobKind, StackJobPolicy};

use super::ansible_steps::root_steps;
use crate::OrchestratorError;
use crate::config::CONFIG_REL;
use crate::discover::Discovery;

pub(crate) fn policies(
    config: &VelnorConfig,
    discovery: &Discovery,
    catalog: &ToolCatalog,
) -> Result<Vec<StackJobPolicy>, OrchestratorError> {
    let Some(ansible) = &config.stacks.ansible else {
        return Ok(Vec::new());
    };
    let timeout = ansible
        .timeout_minutes
        .map_or(Ok(config.workflow.crate_timeout_minutes), u16::try_from)
        .map_err(|_| OrchestratorError::config(CONFIG_REL, "stacks.ansible", "ansible_timeout"))?;
    let timeout = JobTimeout::new(timeout)?;
    let setup = crate::pins::resolve_mise_setup_for_release_target(
        config,
        velnor_actions_contract::ReleaseTarget::LinuxX86_64,
    )?;
    let mut out = Vec::new();
    for root in &discovery.ansible.roots {
        out.push(StackJobPolicy {
            kind: StackJobKind::Ansible,
            id: root.job_id(),
            display_name: root.job_name(),
            timeout,
            services: Vec::new(),
            setup: setup.clone(),
            steps: root_steps(ansible, root, catalog)?,
        });
    }
    out.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(out)
}

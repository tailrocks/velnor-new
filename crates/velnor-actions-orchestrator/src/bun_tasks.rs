//! Resolve `[stacks.bun]` into one job policy per workspace member.

use std::path::Path;

use velnor_actions_bun::{Member, check_package_manager, job_id, member_scripts};
use velnor_actions_contract::{JobTimeout, Step, VelnorConfig};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::verification_jobs::BunMemberPolicy;

use super::bun_steps::{MemberInputs, member_steps};
use crate::OrchestratorError;
use crate::config::CONFIG_REL;
use crate::config::private_wiring::PrivateWiring;
use crate::discover::Discovery;

/// Policies for every member, sorted by job id. `catalog` is the emission
/// catalog; the packageManager agreement check re-reads the concrete pin.
pub(crate) fn policies(
    root: &Path,
    config: &VelnorConfig,
    discovery: &Discovery,
    catalog: &ToolCatalog,
    acquire: Option<&Step>,
    private: Option<&PrivateWiring>,
) -> Result<Vec<BunMemberPolicy>, OrchestratorError> {
    let Some(bun) = &config.stacks.bun else {
        return Ok(Vec::new());
    };
    let concrete = crate::tool_catalog_resolve::resolve_tool_catalog(root, config)?;
    if !concrete.is_project_declared(PinnedTool::Bun) {
        return Err(failure("bun_tool_not_declared"));
    }
    let mut scopes: Vec<String> = config
        .private_sources
        .as_ref()
        .map_or_else(Vec::new, |sources| sources.npm_scopes.clone());
    scopes.sort();
    scopes.dedup();
    let timeout = bun
        .timeout_minutes
        .map_or(Ok(config.workflow.crate_timeout_minutes), u16::try_from)
        .map_err(|_| failure("bun_timeout"))?;
    let timeout = JobTimeout::new(timeout)?;
    let mut out = Vec::new();
    for bun_root in &discovery.bun.roots {
        let workspace = &bun_root.workspace;
        check_package_manager(
            workspace.package_manager.as_deref(),
            concrete.version(PinnedTool::Bun),
        )
        .map_err(|err| failure(err.code()))?;
        let block = match private {
            Some(wiring) => wiring.steps_for_bun(&workspace.root)?,
            None => Vec::new(),
        };
        for member in &workspace.members {
            out.push(policy(
                member,
                &MemberInputs {
                    catalog,
                    root: &workspace.root,
                    member,
                    scripts: &member_scripts(member, bun).map_err(|err| failure(err.code()))?,
                    bunfig: bun_root.bunfig.as_deref(),
                    scopes: &scopes,
                    acquire,
                    private: &block,
                },
                timeout,
            )?);
        }
    }
    out.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(out)
}

fn policy(
    member: &Member,
    inputs: &MemberInputs<'_>,
    timeout: JobTimeout,
) -> Result<BunMemberPolicy, OrchestratorError> {
    Ok(BunMemberPolicy {
        name: member.name.clone(),
        id: job_id(member),
        timeout,
        steps: member_steps(inputs)?,
    })
}

fn failure(problem: &str) -> OrchestratorError {
    OrchestratorError::config(CONFIG_REL, "stacks.bun", problem)
}

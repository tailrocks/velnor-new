//! Resolve `[stacks.bun]` into one job policy per workspace member.

use std::path::Path;

use velnor_actions_bun::{Member, check_package_manager, job_id, member_scripts};
use velnor_actions_contract::config::BunBrowser;
use velnor_actions_contract::config::playwright_lock::{PlaywrightLock, pin_list};
use velnor_actions_contract::{JobTimeout, Step, VelnorConfig};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::verification_jobs::{
    BunMemberPolicy, playwright_install_step,
};

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
    let stack_timeout =
        resolve_timeout(bun.timeout_minutes, config.workflow.crate_timeout_minutes)?;
    let playwright_lock = crate::config::playwright_lock::read_playwright_lock(root)?;
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
            let member_config = bun.members.get(&member.name);
            let browsers = member_config.map_or(&[][..], |member| member.browsers.as_slice());
            let timeout = resolve_timeout(
                member_config.and_then(|member| member.timeout_minutes),
                stack_timeout.minutes(),
            )?;
            let playwright =
                playwright_step(workspace, member, browsers, playwright_lock.as_ref())?;
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
                    playwright: playwright.as_ref(),
                },
                timeout,
            )?);
        }
    }
    out.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(out)
}

/// The configured minutes, or the next less specific bound (hard decision 2:
/// validated, never clamped).
fn resolve_timeout(minutes: Option<u32>, fallback: u16) -> Result<JobTimeout, OrchestratorError> {
    let minutes = minutes
        .map_or(Ok(fallback), u16::try_from)
        .map_err(|_| failure("bun_timeout"))?;
    Ok(JobTimeout::new(minutes)?)
}

/// The Playwright install step of one member, checked against both locks.
fn playwright_step(
    workspace: &velnor_actions_bun::Workspace,
    member: &Member,
    browsers: &[BunBrowser],
    lock: Option<&PlaywrightLock>,
) -> Result<Option<Step>, OrchestratorError> {
    if browsers.is_empty() {
        return Ok(None);
    }
    let member_failure = |problem: &str| failure(&format!("{problem}:{}", member.name));
    let version = member
        .playwright
        .as_deref()
        .ok_or_else(|| member_failure("bun_playwright_missing"))?;
    let core = workspace
        .playwright_core
        .as_ref()
        .filter(|core| core.version == version)
        .ok_or_else(|| member_failure("bun_playwright_core_mismatch"))?;
    let lock = lock.ok_or_else(|| member_failure("playwright_lock_missing"))?;
    let names: Vec<&str> = browsers.iter().map(|browser| browser.as_str()).collect();
    let archives = lock
        .archives_for(version, &core.integrity, &names)
        .map_err(|problem| failure(&format!("{problem}:{}", member.name)))?;
    Ok(Some(playwright_install_step(
        &workspace.root,
        version,
        &pin_list(&archives),
    )?))
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

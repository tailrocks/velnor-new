//! Resolve `[stacks.bun]` into one job policy per workspace member.

use std::path::Path;

use velnor_actions_bun::{
    Member, check_package_manager, check_shard_script, job_id, member_job_scripts, suite_job_id,
};
use velnor_actions_contract::config::playwright_lock::{PlaywrightLock, pin_list};
use velnor_actions_contract::config::{BunBrowser, BunSuiteConfig, BunSuiteReport};
use velnor_actions_contract::{JobTimeout, Step, VelnorConfig};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::verification_jobs::{
    BunLegMatrix, BunMemberPolicy, BunReportPolicy, BunSuitePolicy, playwright_install_step,
};

use super::bun_report_steps::report_steps;
use super::bun_steps::{LegArgs, MemberInputs, member_steps};
use crate::OrchestratorError;
use crate::config::CONFIG_REL;
use crate::config::private_wiring::PrivateWiring;
use crate::discover::Discovery;

/// Every Bun job policy: one per member job and one per declared suite.
#[derive(Debug, Default)]
pub(crate) struct BunPolicies {
    /// Member jobs, sorted by job id; a member whose suites claim every
    /// script has none.
    pub(crate) members: Vec<BunMemberPolicy>,
    /// Suite jobs, sorted by job id.
    pub(crate) suites: Vec<BunSuitePolicy>,
}

/// Policies for every member and suite. `catalog` is the emission
/// catalog; the packageManager agreement check re-reads the concrete pin.
pub(crate) fn policies(
    root: &Path,
    config: &VelnorConfig,
    discovery: &Discovery,
    catalog: &ToolCatalog,
    acquire: Option<&Step>,
    private: Option<&PrivateWiring>,
) -> Result<BunPolicies, OrchestratorError> {
    let Some(bun) = &config.stacks.bun else {
        return Ok(BunPolicies::default());
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
    let mut out = BunPolicies::default();
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
            let member_timeout = resolve_timeout(
                member_config.and_then(|member| member.timeout_minutes),
                stack_timeout.minutes(),
            )?;
            let ctx = MemberCtx {
                catalog,
                workspace,
                member,
                bunfig: bun_root.bunfig.as_deref(),
                scopes: &scopes,
                acquire,
                block: &block,
                lock: playwright_lock.as_ref(),
            };
            let suites = member_config.map(|member| &member.suites);
            if suites.is_some_and(|suites| !suites.is_empty()) && !block.is_empty() {
                return Err(failure(&format!(
                    "bun_suite_private_assets_unsupported:{}",
                    member.name
                )));
            }
            if let Some(scripts) =
                member_job_scripts(member, bun).map_err(|err| failure(err.code()))?
            {
                let playwright = playwright_step(workspace, member, browsers, ctx.lock)?;
                out.members.push(policy(
                    member,
                    &ctx.inputs(&scripts, playwright.as_ref(), None),
                    member_timeout,
                )?);
            }
            for (suite_id, suite) in suites.into_iter().flatten() {
                out.suites
                    .push(suite_policy(&ctx, suite_id, suite, member_timeout)?);
            }
        }
    }
    out.members.sort_by(|left, right| left.id.cmp(&right.id));
    out.suites.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(out)
}

/// Everything one member's jobs share.
struct MemberCtx<'a> {
    catalog: &'a ToolCatalog,
    workspace: &'a velnor_actions_bun::Workspace,
    member: &'a Member,
    bunfig: Option<&'a str>,
    scopes: &'a [String],
    acquire: Option<&'a Step>,
    block: &'a [Step],
    lock: Option<&'a PlaywrightLock>,
}

impl<'a> MemberCtx<'a> {
    fn inputs(
        &self,
        scripts: &'a [String],
        playwright: Option<&'a Step>,
        leg: Option<&'a LegArgs>,
    ) -> MemberInputs<'a> {
        MemberInputs {
            catalog: self.catalog,
            root: &self.workspace.root,
            member: self.member,
            scripts,
            bunfig: self.bunfig,
            scopes: self.scopes,
            acquire: self.acquire,
            private: self.block,
            playwright,
            leg,
        }
    }
}

/// One declared suite's job, with its leg matrix and report job when it
/// shards Playwright.
fn suite_policy(
    ctx: &MemberCtx<'_>,
    suite_id: &str,
    suite: &BunSuiteConfig,
    member_timeout: JobTimeout,
) -> Result<BunSuitePolicy, OrchestratorError> {
    let member = ctx.member;
    let playwright = playwright_step(ctx.workspace, member, &suite.browsers, ctx.lock)?;
    let id = suite_job_id(member, suite_id);
    let timeout = resolve_timeout(suite.timeout_minutes, member_timeout.minutes())?;
    let mut matrix = None;
    let mut leg = None;
    let mut report = None;
    if let Some(table) = &suite.playwright {
        for script in &suite.scripts {
            check_shard_script(member, script).map_err(|err| failure(err.code()))?;
        }
        let legs = BunLegMatrix::new(&table.projects, table.shards, table.max_parallel);
        leg = Some(LegArgs {
            projects: legs.has_projects(),
            sharded: legs.is_sharded(),
            fail_on_flaky: table.fail_on_flaky,
            job_id: id.clone(),
            results_retention_days: table.results_retention_days,
        });
        matrix = Some(legs);
        if table.report == BunSuiteReport::Merge {
            report = Some(BunReportPolicy {
                timeout,
                steps: report_steps(
                    &ctx.inputs(&suite.scripts, None, None),
                    &id,
                    table.report_retention_days,
                )?,
            });
        }
    }
    let steps = member_steps(&ctx.inputs(&suite.scripts, playwright.as_ref(), leg.as_ref()))?;
    Ok(BunSuitePolicy {
        member: member.name.clone(),
        suite: suite_id.to_owned(),
        id,
        timeout,
        required: suite.required,
        steps,
        matrix,
        report,
    })
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

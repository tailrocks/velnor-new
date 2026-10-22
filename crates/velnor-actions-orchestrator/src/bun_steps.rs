//! Steps of one Bun member job (hard decision 6).
//!
//! `Prepare pinned tools` (Bun), `Verify Bun workspace`, the install, then
//! one background step per script joined by the renderer's `wait-all`.
//! Every Bun invocation runs under `mise exec` with the isolation env, so the
//! repository cannot select another Bun, and no step carries a credential.

use std::collections::BTreeMap;

use velnor_actions_bun::{Member, velnor_bunfig};
use velnor_actions_contract::config::playwright_lock::PLAYWRIGHT_HOST_PLATFORM;
use velnor_actions_contract::{Step, StepRole};
use velnor_actions_mise::{
    PREPARE_PINNED_TOOLS_STEP, PinnedTool, PinnedToolExec, PreparePinnedTools, ToolCatalog,
    ToolHomes,
};
use velnor_actions_workflow_renderer::verification_jobs::{
    bun_blob_upload_step, bun_registry_step, bun_results_upload_step,
};
use velnor_actions_workflow_renderer::{ambient_shell_step, shell_step};

use crate::OrchestratorError;
use crate::config::playwright_install::BROWSERS_SUBDIR;
use crate::utf8::{strings_of, strings_of_env};
use crate::workflow_task_jobs::stack_family_cache;

/// Cache directory of Bun's package cache (`BUN_INSTALL_CACHE_DIR`).
const CACHE_DIR: &str = "${{ runner.temp }}/velnor/bun-cache";
/// Where the Velnor-written install configuration lives at run time.
const CONFIG_DIR: &str = "$RUNNER_TEMP/velnor/bun";
const VERIFY_NAME: &str = "Verify Bun workspace";
const INSTALL_NAME: &str = "Install dependencies";
const LIFECYCLE_NAME: &str = "Install dependencies (lifecycle phase)";

/// Inputs shared by every step of one member.
pub(crate) struct MemberInputs<'a> {
    /// Emission catalog: Bun renders as `${VELNOR_TOOL_BUN}`.
    pub(crate) catalog: &'a ToolCatalog,
    /// Workspace root, repository-relative (`""` for the root).
    pub(crate) root: &'a str,
    /// The member.
    pub(crate) member: &'a Member,
    /// Scripts to run, in order.
    pub(crate) scripts: &'a [String],
    /// Repository `bunfig.toml` text.
    pub(crate) bunfig: Option<&'a str>,
    /// Private npm scopes (sorted); non-empty selects the two-phase install.
    pub(crate) scopes: &'a [String],
    /// Staged helper step the registry phase needs (consumer policy).
    pub(crate) acquire: Option<&'a Step>,
    /// Mint, fetch and revoke steps for pinned private release assets (HD-7);
    /// empty when the member consumes none. They run before every other step.
    pub(crate) private: &'a [Step],
    /// Hash-verified Playwright browser install, when the member declares browsers.
    pub(crate) playwright: Option<&'a Step>,
    /// Matrix leg arguments, when the job is a sharded Playwright suite.
    pub(crate) leg: Option<&'a LegArgs>,
}

/// Which flags a sharded suite's leg appends to its one Playwright script,
/// and where its artifacts go.
#[derive(Debug, Clone)]
pub(crate) struct LegArgs {
    /// Legs pass `--project=$VELNOR_PW_PROJECT`.
    pub(crate) projects: bool,
    /// Legs pass `--shard=$VELNOR_PW_SHARD`.
    pub(crate) sharded: bool,
    /// Legs pass `--fail-on-flaky-tests`.
    pub(crate) fail_on_flaky: bool,
    /// The suite job id, which names every leg artifact.
    pub(crate) job_id: String,
    /// Retention of a failed leg's `test-results`.
    pub(crate) results_retention_days: u16,
}

/// Leg env: the project of this leg (`matrix.project`).
pub(crate) const PW_PROJECT_ENV: &str = "VELNOR_PW_PROJECT";
/// Leg env: `<shard>/<total>` of this leg.
pub(crate) const PW_SHARD_ENV: &str = "VELNOR_PW_SHARD";
/// Playwright's blob reporter output file; Velnor fixes one path per leg.
pub(crate) const PW_BLOB_FILE_ENV: &str = "PLAYWRIGHT_BLOB_OUTPUT_FILE";
/// Directory of every leg blob, under the runner's temp directory.
pub(crate) const PW_BLOB_DIR: &str = "velnor/pw-blob";

/// Every step after checkout, in execution order.
pub(crate) fn member_steps(input: &MemberInputs<'_>) -> Result<Vec<Step>, OrchestratorError> {
    let staged =
        !input.scopes.is_empty() || !input.private.is_empty() || input.playwright.is_some();
    let mut steps = Vec::new();
    if staged {
        steps.extend(input.acquire.cloned());
    }
    steps.extend(input.private.iter().cloned());
    steps.push(prepare_step(input.catalog)?);
    steps.push(verify_step(input)?);
    let bun = stack_family_cache::bun_install(input.root)?;
    let mut plans = vec![bun.clone()];
    let browsers = input
        .playwright
        .map(|_| stack_family_cache::playwright_browsers());
    plans.extend(browsers.clone());
    steps.extend(stack_family_cache::prelude(&plans)?);
    if input.scopes.is_empty() {
        steps.push(install_step(input)?);
    } else {
        steps.push(bun_registry_step(
            input.root,
            input.scopes,
            &step_env(input.catalog)?,
        )?);
        steps.push(lifecycle_step(input)?);
    }
    steps.extend(stack_family_cache::after_producer(&bun)?);
    steps.extend(input.playwright.cloned());
    if let Some(browsers) = &browsers {
        steps.extend(stack_family_cache::after_producer(browsers)?);
    }
    for script in input.scripts {
        let mut step = script_step(input, script)?;
        step.parallel = true;
        steps.push(step);
    }
    if let Some(leg) = input.leg {
        steps.push(super::bun_report_steps::blob_verify_step(input, leg)?);
        steps.push(bun_blob_upload_step(&leg.job_id)?);
        steps.push(bun_results_upload_step(
            &leg.job_id,
            &input.member.dir,
            leg.results_retention_days,
        )?);
    }
    Ok(steps)
}

fn contract(problem: impl Into<String>) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: problem.into(),
    }
}

pub(crate) fn prepare_step(catalog: &ToolCatalog) -> Result<Step, OrchestratorError> {
    let prepare = PreparePinnedTools::new(vec![PinnedTool::Bun], ToolHomes::runner_temp())
        .map_err(|err| contract(err.to_string()))?;
    let run = strings_of(prepare.argv(catalog)).map_err(contract)?;
    let env = strings_of_env(&prepare.env_without_homes()).map_err(contract)?;
    let mut step = ambient_shell_step(PREPARE_PINNED_TOOLS_STEP, run, env)?;
    step.role = Some(StepRole::PreparePinnedTools);
    Ok(step)
}

pub(crate) fn step_env(
    catalog: &ToolCatalog,
) -> Result<BTreeMap<String, String>, OrchestratorError> {
    let extra = BTreeMap::from([("BUN_INSTALL_CACHE_DIR".to_owned(), CACHE_DIR.to_owned())]);
    crate::matrix_step::task_step_env(catalog, &extra, false)
}

pub(crate) fn bash(script: &str) -> Vec<String> {
    vec!["bash".to_owned(), "-c".to_owned(), script.to_owned()]
}

/// Single-quote `text` for the shell; project tool references stay expandable.
fn quote(text: &str) -> String {
    if text.contains("${VELNOR_TOOL_") {
        format!("\"{text}\"")
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
}

/// `mise exec bun@<pin> -- bun <args>` as shell words.
pub(crate) fn bun_words(
    catalog: &ToolCatalog,
    args: &[String],
) -> Result<String, OrchestratorError> {
    let request = PinnedToolExec::new(
        vec![PinnedTool::Bun],
        std::ffi::OsStr::new("bun"),
        args.iter().map(std::ffi::OsString::from).collect(),
    )
    .map_err(|err| contract(err.to_string()))?;
    let words = strings_of(request.argv(catalog)).map_err(contract)?;
    Ok(words
        .iter()
        .map(|word| quote(word))
        .collect::<Vec<_>>()
        .join(" "))
}

pub(crate) fn dir_arg(dir: &str) -> String {
    if dir.is_empty() {
        ".".to_owned()
    } else {
        dir.to_owned()
    }
}

pub(crate) fn verify_step(input: &MemberInputs<'_>) -> Result<Step, OrchestratorError> {
    let root = quote(&dir_arg(input.root));
    let script = format!(
        "set -euo pipefail; cd -P -- \"$GITHUB_WORKSPACE\"; cd -- {root}; test -f bun.lock; test ! -L bun.lock; \
for f in package-lock.json npm-shrinkwrap.json yarn.lock pnpm-lock.yaml bun.lockb; do test ! -e \"$f\"; done; \
pm=''; while IFS= read -r line; do case \"$line\" in *'\"packageManager\"'*) pm=\"$line\" ;; esac; done < package.json; \
case \"$pm\" in ''|*\"\\\"bun@${{VELNOR_TOOL_BUN}}\\\"\"*|*\"\\\"bun@${{VELNOR_TOOL_BUN}}+\"*) ;; \
*) echo 'velnor: packageManager disagrees with the Mise-declared Bun' >&2; exit 1 ;; esac"
    );
    Ok(shell_step(
        VERIFY_NAME,
        bash(&script),
        step_env(input.catalog)?,
    )?)
}

pub(crate) fn install_step(input: &MemberInputs<'_>) -> Result<Step, OrchestratorError> {
    let config = velnor_bunfig(input.bunfig, &[], false).map_err(|err| contract(err.code()))?;
    let install = bun_words(
        input.catalog,
        &[
            "install".to_owned(),
            "--frozen-lockfile".to_owned(),
            format!("--cwd={}", dir_arg(input.root)),
            "--config=@CONFIG@".to_owned(),
        ],
    )?;
    let lines = config.lines().map(quote).collect::<Vec<_>>().join(" ");
    let install = install.replace("'--config=@CONFIG@'", "\"--config=$cfg\"");
    let script = format!(
        "set -euo pipefail; cd -P -- \"$GITHUB_WORKSPACE\"; d=\"{CONFIG_DIR}\"; mkdir -p \"$d\"; cfg=\"$d/bunfig.toml\"; \
printf '%s\\n' {lines} > \"$cfg\"; {install}"
    );
    Ok(shell_step(
        INSTALL_NAME,
        bash(&script),
        step_env(input.catalog)?,
    )?)
}

/// Token-free second phase: rebuild `node_modules` from the warm cache so the
/// trusted lifecycle scripts run, with the config the registry phase wrote.
pub(crate) fn lifecycle_step(input: &MemberInputs<'_>) -> Result<Step, OrchestratorError> {
    let root = quote(&dir_arg(input.root));
    let install = bun_words(
        input.catalog,
        &[
            "install".to_owned(),
            "--frozen-lockfile".to_owned(),
            format!("--cwd={}", dir_arg(input.root)),
            "--config=@CONFIG@".to_owned(),
        ],
    )?
    .replace(
        "'--config=@CONFIG@'",
        "\"--config=$RUNNER_TEMP/velnor/bun/lifecycle.toml\"",
    );
    let script = format!(
        "set -euo pipefail; cd -P -- \"$GITHUB_WORKSPACE\"; find {root} -name node_modules -type d -prune -exec rm -rf {{}} +; {install}"
    );
    Ok(shell_step(
        LIFECYCLE_NAME,
        bash(&script),
        step_env(input.catalog)?,
    )?)
}

/// Environment that points Playwright at the Velnor-installed browsers only.
fn browser_env(input: &MemberInputs<'_>) -> Result<BTreeMap<String, String>, OrchestratorError> {
    let mut env = step_env(input.catalog)?;
    if input.playwright.is_some() {
        env.extend([
            (
                "PLAYWRIGHT_BROWSERS_PATH".to_owned(),
                format!("${{{{ runner.temp }}}}/{BROWSERS_SUBDIR}"),
            ),
            (
                "PLAYWRIGHT_HOST_PLATFORM_OVERRIDE".to_owned(),
                PLAYWRIGHT_HOST_PLATFORM.to_owned(),
            ),
            (
                "PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD".to_owned(),
                "1".to_owned(),
            ),
        ]);
    }
    Ok(env)
}

fn script_step(input: &MemberInputs<'_>, script: &str) -> Result<Step, OrchestratorError> {
    let mut args = vec![
        "run".to_owned(),
        format!("--cwd={}", dir_arg(&input.member.dir)),
        script.to_owned(),
    ];
    let mut env = browser_env(input)?;
    if let Some(leg) = input.leg {
        // Matrix values reach the shell only through env, never the run text.
        if leg.projects {
            args.push("@PW_PROJECT@".to_owned());
            env.insert(
                PW_PROJECT_ENV.to_owned(),
                "${{ matrix.project }}".to_owned(),
            );
        }
        if leg.sharded {
            args.push("@PW_SHARD@".to_owned());
            env.insert(
                PW_SHARD_ENV.to_owned(),
                "${{ matrix.shard }}/${{ matrix.total }}".to_owned(),
            );
        }
        if leg.fail_on_flaky {
            args.push("--fail-on-flaky-tests".to_owned());
        }
        env.insert(
            PW_BLOB_FILE_ENV.to_owned(),
            format!("${{{{ runner.temp }}}}/{PW_BLOB_DIR}/leg-${{{{ matrix.leg }}}}.zip"),
        );
    }
    let run = bun_words(input.catalog, &args)?
        .replace(
            "'@PW_PROJECT@'",
            &format!("\"--project=${PW_PROJECT_ENV}\""),
        )
        .replace("'@PW_SHARD@'", &format!("\"--shard=${PW_SHARD_ENV}\""));
    let body = format!("set -euo pipefail; cd -P -- \"$GITHUB_WORKSPACE\"; {run}");
    Ok(shell_step(&step_name(script), bash(&body), env)?)
}

/// Display name of a script step.
pub(crate) fn step_name(script: &str) -> String {
    match script {
        "typecheck" => "Typecheck".to_owned(),
        "lint" => "Lint".to_owned(),
        "test" => "Test".to_owned(),
        "build" => "Build".to_owned(),
        other => format!("Script {other}"),
    }
}

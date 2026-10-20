//! Steps of one Bun member job (hard decision 6).
//!
//! `Prepare pinned tools` (Bun), `Verify Bun workspace`, the install, then
//! one background step per script joined by the renderer's `wait-all`.
//! Every Bun invocation runs under `mise exec` with the isolation env, so the
//! repository cannot select another Bun, and no step carries a credential.

use std::collections::BTreeMap;

use velnor_actions_bun::{Member, velnor_bunfig};
use velnor_actions_contract::{Step, StepRole};
use velnor_actions_mise::{
    PREPARE_PINNED_TOOLS_STEP, PinnedTool, PinnedToolExec, PreparePinnedTools, ToolCatalog,
    ToolHomes,
};
use velnor_actions_workflow_renderer::{ambient_shell_step, shell_step};

use crate::OrchestratorError;
use crate::utf8::{strings_of, strings_of_env};

/// Cache directory of Bun's package cache (`BUN_INSTALL_CACHE_DIR`).
const CACHE_DIR: &str = "${{ runner.temp }}/velnor/bun-cache";
/// Where the Velnor-written install configuration lives at run time.
const CONFIG_DIR: &str = "$RUNNER_TEMP/velnor/bun";
const VERIFY_NAME: &str = "Verify Bun workspace";
const INSTALL_NAME: &str = "Install dependencies";

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
}

/// Every step after checkout, in execution order.
pub(crate) fn member_steps(input: &MemberInputs<'_>) -> Result<Vec<Step>, OrchestratorError> {
    let mut steps = vec![
        prepare_step(input.catalog)?,
        verify_step(input)?,
        install_step(input)?,
    ];
    for script in input.scripts {
        let mut step = script_step(input, script)?;
        step.parallel = true;
        steps.push(step);
    }
    Ok(steps)
}

fn contract(problem: impl Into<String>) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: problem.into(),
    }
}

fn prepare_step(catalog: &ToolCatalog) -> Result<Step, OrchestratorError> {
    let prepare = PreparePinnedTools::new(vec![PinnedTool::Bun], ToolHomes::runner_temp())
        .map_err(|err| contract(err.to_string()))?;
    let run = strings_of(prepare.argv(catalog)).map_err(contract)?;
    let env = strings_of_env(&prepare.env_without_homes()).map_err(contract)?;
    let mut step = ambient_shell_step(PREPARE_PINNED_TOOLS_STEP, run, env)?;
    step.role = Some(StepRole::PreparePinnedTools);
    Ok(step)
}

fn step_env(catalog: &ToolCatalog) -> Result<BTreeMap<String, String>, OrchestratorError> {
    let extra = BTreeMap::from([("BUN_INSTALL_CACHE_DIR".to_owned(), CACHE_DIR.to_owned())]);
    crate::matrix_step::task_step_env(catalog, &extra, false)
}

fn bash(script: &str) -> Vec<String> {
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
fn bun_words(catalog: &ToolCatalog, args: &[String]) -> Result<String, OrchestratorError> {
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

fn dir_arg(dir: &str) -> String {
    if dir.is_empty() {
        ".".to_owned()
    } else {
        dir.to_owned()
    }
}

fn verify_step(input: &MemberInputs<'_>) -> Result<Step, OrchestratorError> {
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

fn install_step(input: &MemberInputs<'_>) -> Result<Step, OrchestratorError> {
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

fn script_step(input: &MemberInputs<'_>, script: &str) -> Result<Step, OrchestratorError> {
    let run = bun_words(
        input.catalog,
        &[
            "run".to_owned(),
            format!("--cwd={}", dir_arg(&input.member.dir)),
            script.to_owned(),
        ],
    )?;
    let body = format!("set -euo pipefail; cd -P -- \"$GITHUB_WORKSPACE\"; {run}");
    Ok(shell_step(
        &step_name(script),
        bash(&body),
        step_env(input.catalog)?,
    )?)
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

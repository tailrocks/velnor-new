//! Steps of one dbt project job.
//!
//! `Install dbt-fusion` (a checksum-verified download), `dbt deps`, then one
//! background step per check joined by the renderer's `wait-all`. The archive
//! URL is rebuilt from the project-declared version and must equal the URL the
//! project's `mise.lock` row records; the lock's SHA-256 is the only trust
//! anchor. No step carries a credential: `dbt deps` talks to the public
//! package hub only.

use std::collections::BTreeMap;

use velnor_actions_contract::config::DbtCheck;
use velnor_actions_contract::{Step, VelnorConfig};
use velnor_actions_dbt::FUSION_HOST;
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::shell_step;

use super::stack_support::{LockedArchive, bash, quote};
use crate::OrchestratorError;

/// Where the verified binary lives at run time.
const FUSION_DIR: &str = "$RUNNER_TEMP/velnor/dbt-fusion/bin";
const INSTALL_NAME: &str = "Install dbt-fusion";
const DEPS_NAME: &str = "dbt deps";
const LINUX_X64_TRIPLE: &str = "x86_64-unknown-linux-gnu";

fn contract(problem: impl Into<String>) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: problem.into(),
    }
}

/// The step environment: Velnor isolation plus the declared `DBT_*` literals.
fn step_env(
    catalog: &ToolCatalog,
    config: &velnor_actions_contract::config::DbtStackConfig,
) -> Result<BTreeMap<String, String>, OrchestratorError> {
    let mut extra: BTreeMap<String, String> = config.env.clone();
    extra.insert("DBT_PROJECT_DIR".to_owned(), ".".to_owned());
    extra.insert("DBT_PROFILES_DIR".to_owned(), ".".to_owned());
    extra.insert("DO_NOT_TRACK".to_owned(), "1".to_owned());
    extra.insert(
        "DBT_SEND_ANONYMOUS_USAGE_STATS".to_owned(),
        "false".to_owned(),
    );
    crate::matrix_step::task_step_env(catalog, &extra, false)
}

fn install_step(
    catalog: &ToolCatalog,
    config: &velnor_actions_contract::config::DbtStackConfig,
) -> Result<Step, OrchestratorError> {
    let url_check = format!(
        "want=\"https://{FUSION_HOST}/fs/cli/fs-v${{VELNOR_TOOL_DBT_FUSION}}-{LINUX_X64_TRIPLE}.tar.gz\"; \
if [ \"${{url:-}}\" != \"$want\" ]; then echo 'velnor: the mise.lock dbt-fusion url is not the admitted archive' >&2; exit 1; fi"
    );
    let archive = LockedArchive {
        key: "dbt-fusion",
        version_var: "VELNOR_TOOL_DBT_FUSION",
        dest: FUSION_DIR,
        url_check: &url_check,
        tar_args: "",
        binary: "dbt",
    };
    Ok(shell_step(
        INSTALL_NAME,
        bash(&archive.script()),
        step_env(catalog, config)?,
    )?)
}

fn dbt_step(
    name: &str,
    dir: &str,
    args: &str,
    catalog: &ToolCatalog,
    config: &velnor_actions_contract::config::DbtStackConfig,
) -> Result<Step, OrchestratorError> {
    let script = format!(
        "set -euo pipefail; cd -P -- \"$GITHUB_WORKSPACE\"; cd -- {dir}; \"{FUSION_DIR}/dbt\" {args}",
        dir = quote(dir),
    );
    Ok(shell_step(name, bash(&script), step_env(catalog, config)?)?)
}

/// Display name and arguments of one check step.
fn check_command(check: DbtCheck) -> (&'static str, &'static str) {
    match check {
        DbtCheck::Parse => ("dbt parse", "parse"),
        DbtCheck::Compile => ("dbt compile", "compile --static-analysis off"),
    }
}

/// Every step after checkout, in execution order.
pub(crate) fn project_steps(
    config: &VelnorConfig,
    dir: &str,
    catalog: &ToolCatalog,
) -> Result<Vec<Step>, OrchestratorError> {
    let dbt = config
        .stacks
        .dbt
        .as_ref()
        .ok_or_else(|| contract("dbt_stack_not_configured"))?;
    let mut steps = vec![
        install_step(catalog, dbt)?,
        dbt_step(DEPS_NAME, dir, "deps", catalog, dbt)?,
    ];
    for check in dbt.effective_checks() {
        let (name, args) = check_command(check);
        let mut step = dbt_step(name, dir, args, catalog, dbt)?;
        step.parallel = true;
        steps.push(step);
    }
    Ok(steps)
}

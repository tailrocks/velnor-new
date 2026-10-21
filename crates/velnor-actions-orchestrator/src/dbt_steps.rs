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

use crate::OrchestratorError;

/// Where the verified binary lives at run time.
const FUSION_DIR: &str = "$RUNNER_TEMP/velnor/dbt-fusion";
const INSTALL_NAME: &str = "Install dbt-fusion";
const DEPS_NAME: &str = "dbt deps";
/// Lock table of the linux-x64 row (the hosted runner platform).
const LOCK_TABLE: &str = "[tools.dbt-fusion.\"platforms.linux-x64\"]";
const LINUX_X64_TRIPLE: &str = "x86_64-unknown-linux-gnu";

pub(crate) fn bash(script: &str) -> Vec<String> {
    vec!["bash".to_owned(), "-c".to_owned(), script.to_owned()]
}

/// Single-quote a validated repository path or literal for the shell.
pub(crate) fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

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

/// The awk program that prints `<url> <sha256>` of the declared version's row.
const ROW_PROGRAM: &str = "/^\\[\\[tools\\.dbt-fusion\\]\\]/ { ver = \"\"; row = 0; next } \
/^\\[/ { row = ($0 == table && ver == want); next } \
/^version = \"/ && !row { ver = $0; sub(/^version = \"/, \"\", ver); sub(/\"$/, \"\", ver); next } \
row && /^url = \"/ { url = $0; sub(/^url = \"/, \"\", url); sub(/\"$/, \"\", url) } \
row && /^checksum = \"sha256:/ { sum = $0; sub(/^checksum = \"sha256:/, \"\", sum); sub(/\"$/, \"\", sum) } \
END { print url, sum }";

fn install_step(
    catalog: &ToolCatalog,
    config: &velnor_actions_contract::config::DbtStackConfig,
) -> Result<Step, OrchestratorError> {
    let script = format!(
        "set -euo pipefail; cd -P -- \"$GITHUB_WORKSPACE\"; d=\"{FUSION_DIR}\"; rm -rf \"$d\"; mkdir -p \"$d/bin\"; \
lock=mise.lock; if [ ! -f \"$lock\" ]; then lock=.mise.lock; fi; test -f \"$lock\"; test ! -L \"$lock\"; \
want=\"https://{FUSION_HOST}/fs/cli/fs-v${{VELNOR_TOOL_DBT_FUSION}}-{LINUX_X64_TRIPLE}.tar.gz\"; \
awk -v table={table} -v want=\"${{VELNOR_TOOL_DBT_FUSION}}\" {program} \"$lock\" > \"$d/row\"; \
IFS=' ' read -r url sum < \"$d/row\" || true; \
if [ \"${{url:-}}\" != \"$want\" ]; then echo 'velnor: the mise.lock dbt-fusion url is not the admitted archive' >&2; exit 1; fi; \
case \"${{sum:-}}\" in ''|*[!0-9a-f]*) echo 'velnor: the mise.lock dbt-fusion row has no sha256 checksum' >&2; exit 1 ;; esac; \
if [ \"${{#sum}}\" -ne 64 ]; then echo 'velnor: the mise.lock dbt-fusion checksum is malformed' >&2; exit 1; fi; \
curl --fail --silent --show-error --location --proto '=https' --proto-redir '=https' --tlsv1.2 --retry 3 -o \"$d/fusion.tar.gz\" \"$url\"; \
printf '%s  %s\\n' \"$sum\" \"$d/fusion.tar.gz\" > \"$d/fusion.sha256\"; sha256sum -c \"$d/fusion.sha256\"; \
tar -xzf \"$d/fusion.tar.gz\" -C \"$d/bin\"; rm -f \"$d/fusion.tar.gz\"; test -x \"$d/bin/dbt\"",
        table = quote(LOCK_TABLE),
        program = quote(ROW_PROGRAM),
    );
    Ok(shell_step(
        INSTALL_NAME,
        bash(&script),
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
        "set -euo pipefail; cd -P -- \"$GITHUB_WORKSPACE\"; cd -- {dir}; \"{FUSION_DIR}/bin/dbt\" {args}",
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

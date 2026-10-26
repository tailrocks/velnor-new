// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::config::DbtCheck;
use velnor_actions_contract::{Step, VelnorConfig};
use velnor_actions_dbt::{FUSION_HOST, Project};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::shell_step;

use super::stack_support::{LockedArchive, bash, quote};
use crate::OrchestratorError;
use crate::workflow_task_jobs::stack_family_cache;

const FUSION_DIR: &str = "$RUNNER_TEMP/velnor/dbt-fusion/bin";
const INSTALL_NAME: &str = "Install dbt-fusion";
const DEPS_NAME: &str = "dbt deps";
const LINUX_X64_TRIPLE: &str = "x86_64-unknown-linux-gnu";
const EXPERIMENTAL_ADAPTERS_ENV: &str = "DBT_ALLOW_EXPERIMENTAL_ADAPTERS";

fn contract(problem: impl Into<String>) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: problem.into(),
    }
}

fn step_env(
    catalog: &ToolCatalog,
    config: &velnor_actions_contract::config::DbtStackConfig,
    project: &Project,
) -> Result<BTreeMap<String, String>, OrchestratorError> {
    let mut extra: BTreeMap<String, String> = config.env.clone();
    if project.postgres_adapter {
        extra.insert(EXPERIMENTAL_ADAPTERS_ENV.to_owned(), "true".to_owned());
    }
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
    project: &Project,
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
        step_env(catalog, config, project)?,
    )?)
}

fn dbt_step(
    name: &str,
    dir: &str,
    args: &str,
    catalog: &ToolCatalog,
    config: &velnor_actions_contract::config::DbtStackConfig,
    project: &Project,
) -> Result<Step, OrchestratorError> {
    let script = format!(
        "set -euo pipefail; cd -P -- \"$GITHUB_WORKSPACE\"; cd -- {dir}; \"{FUSION_DIR}/dbt\" {args}",
        dir = quote(dir),
    );
    Ok(shell_step(
        name,
        bash(&script),
        step_env(catalog, config, project)?,
    )?)
}

fn check_command(check: DbtCheck) -> (&'static str, &'static str) {
    match check {
        DbtCheck::Parse => ("dbt parse", "parse"),
        DbtCheck::Compile => ("dbt compile", "compile --static-analysis off"),
    }
}

fn packages_copy_step(
    stage: bool,
    plan: &velnor_actions_workflow_renderer::steps::FamilyCachePlan,
    dir: &str,
    catalog: &ToolCatalog,
    config: &velnor_actions_contract::config::DbtStackConfig,
    project: &Project,
) -> Result<Step, OrchestratorError> {
    let family = plan
        .family
        .spec()
        .shell_paths(plan.locator.as_deref())
        .join("");
    let (name, copy) = if stage {
        (
            "Stage dbt packages",
            format!(
                "if [ -d \"{family}\" ] && [ ! -L \"{family}\" ]; then rm -rf \"$p\"; mkdir -p \"$p\"; cp -a \"{family}/.\" \"$p/\"; fi"
            ),
        )
    } else {
        (
            "Stash dbt packages",
            format!(
                "if [ -d \"$p\" ] && [ ! -L \"$p\" ]; then rm -rf \"{family}\"; mkdir -p \"{family}\"; cp -a \"$p/.\" \"{family}/\"; fi"
            ),
        )
    };
    let script = format!(
        "set -euo pipefail; cd -P -- \"$GITHUB_WORKSPACE\"; cd -- {dir}; p=\"$PWD/dbt_packages\"; {copy}",
        dir = quote(dir),
    );
    Ok(shell_step(
        name,
        bash(&script),
        step_env(catalog, config, project)?,
    )?)
}

pub(crate) fn project_steps(
    config: &VelnorConfig,
    project: &Project,
    catalog: &ToolCatalog,
) -> Result<Vec<Step>, OrchestratorError> {
    let dir = project.dir.as_str();
    let dbt = config
        .stacks
        .dbt
        .as_ref()
        .ok_or_else(|| contract("dbt_stack_not_configured"))?;
    let packages = stack_family_cache::dbt_packages(dir)?;
    let mut steps = vec![install_step(catalog, dbt, project)?];
    steps.extend(stack_family_cache::prelude(std::slice::from_ref(
        &packages,
    ))?);
    steps.push(packages_copy_step(
        true, &packages, dir, catalog, dbt, project,
    )?);
    steps.push(dbt_step(DEPS_NAME, dir, "deps", catalog, dbt, project)?);
    steps.push(packages_copy_step(
        false, &packages, dir, catalog, dbt, project,
    )?);
    steps.extend(stack_family_cache::after_producer(&packages)?);
    for check in dbt.effective_checks() {
        let (name, args) = check_command(check);
        let mut step = dbt_step(name, dir, args, catalog, dbt, project)?;
        step.parallel = true;
        steps.push(step);
    }
    Ok(steps)
}

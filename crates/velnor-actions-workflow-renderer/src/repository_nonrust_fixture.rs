// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::step_identity::REPOSITORY_NONRUST_FIXTURE_OPERATION;
use velnor_actions_contract::{Job, Permissions, Step, StepKind, StepRole, WorkflowPolicy};
use velnor_actions_mise::{PinnedTool, ToolCatalog, catalog::BUN_VERSION};

use crate::{RenderError, yaml::Yaml};

const NAME: &str = "Prepare non-Rust tools fixture";
const PACKAGE: &str = "velnor-actions-orchestrator";
const MANIFEST: &str = "crates/velnor-actions-orchestrator/Cargo.toml";
const JOB: &str = "rust-velnor-actions-orchestrator";
const CONDITION: &str = "github.repository == 'tailrocks/velnor-new'";
const TOKEN: &str = "VELNOR_FIXTURE_ORIGIN_TOKEN";

fn invalid() -> RenderError {
    RenderError::InvalidWorkflow("repository_nonrust_fixture_scope".to_owned())
}

fn environment() -> BTreeMap<String, String> {
    let mut env = crate::toolchain_env::credential_scrub();
    env.insert(TOKEN.to_owned(), "${{ github.token }}".to_owned());
    env
}

pub fn repository_nonrust_fixture_step(
    policy: WorkflowPolicy,
    package: &str,
    manifest: &str,
    catalog: &ToolCatalog,
) -> Result<Step, RenderError> {
    if policy != WorkflowPolicy::VelnorRepositoryV1
        || package != PACKAGE
        || manifest != MANIFEST
        || !(catalog.version(PinnedTool::Bun) == BUN_VERSION
            || catalog.is_emission() && catalog.version(PinnedTool::Bun) == "${VELNOR_TOOL_BUN}")
    {
        return Err(invalid());
    }
    Ok(Step {
        parallel: false,
        name: NAME.to_owned(),
        id: None,
        role: Some(StepRole::RepositoryNonrustFixture),
        condition: Some(CONDITION.to_owned()),
        kind: StepKind::Internal {
            operation: REPOSITORY_NONRUST_FIXTURE_OPERATION.to_owned(),
            env: environment(),
        },
    })
}

fn applies(step: &Step) -> bool {
    step.role == Some(StepRole::RepositoryNonrustFixture)
        || matches!(&step.kind, StepKind::Internal { operation, .. } if operation == REPOSITORY_NONRUST_FIXTURE_OPERATION)
}

fn validate(job: &str, step: &Step) -> Result<(), RenderError> {
    let StepKind::Internal { operation, env } = &step.kind else {
        return Err(invalid());
    };
    if job != JOB
        || step.name != NAME
        || step.role != Some(StepRole::RepositoryNonrustFixture)
        || operation != REPOSITORY_NONRUST_FIXTURE_OPERATION
        || env != &environment()
        || step.condition.as_deref() != Some(CONDITION)
        || step.parallel
        || step.id.is_some()
    {
        return Err(invalid());
    }
    Ok(())
}

pub(crate) fn validate_jobs(
    jobs: &BTreeMap<String, Job>,
    policy: WorkflowPolicy,
    workflow_permissions: &Permissions,
) -> Result<(), RenderError> {
    for (id, job) in jobs {
        for step in job.steps.iter().filter(|step| applies(step)) {
            if policy != WorkflowPolicy::VelnorRepositoryV1 {
                return Err(invalid());
            }
            if workflow_permissions != &Permissions::default()
                || job
                    .permissions
                    .as_ref()
                    .is_some_and(|permissions| permissions != &Permissions::default())
            {
                return Err(invalid());
            }
            validate(id, step)?;
        }
    }
    Ok(())
}

pub(crate) fn render(job: &str, step: &Step, composite: bool) -> Result<Option<Yaml>, RenderError> {
    if !applies(step) {
        return Ok(None);
    }
    validate(job, step)?;
    if composite {
        return Err(invalid());
    }
    Ok(Some(Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(NAME)),
        ("if".to_owned(), Yaml::str(CONDITION)),
        ("shell".to_owned(), Yaml::str(crate::VERIFY_VERIFIER_SHELL)),
        (
            "env".to_owned(),
            crate::document_steps::string_map_yaml(&environment()),
        ),
        ("run".to_owned(), Yaml::str(launcher())),
    ])))
}

fn launcher() -> String {
    let mut denied = crate::toolchain_env::credential_scrub()
        .into_keys()
        .collect::<Vec<_>>();
    denied.extend(crate::toolchain_env::CREDENTIAL_UNSET_VARS.map(str::to_owned));
    denied.sort();
    denied.dedup();
    format!(
        r#"set -euo pipefail
set +a
if [ "${{GITHUB_REPOSITORY:-}}" != tailrocks/velnor-new ]; then printf '%s\n' 'fixture repository is not admitted' >&2; exit 1; fi
case "${{GITHUB_WORKSPACE:-}}" in /*) ;; *) printf '%s\n' 'fixture workspace must be absolute' >&2; exit 1 ;; esac
cd -P -- "$GITHUB_WORKSPACE"
fixture_workspace=$PWD
unset fixture_origin_credential
fixture_origin_credential=${{VELNOR_FIXTURE_ORIGIN_TOKEN:?}}
unset VELNOR_FIXTURE_ORIGIN_TOKEN
unset {denied}
unset BUN_OPTIONS NODE_OPTIONS NODE_PATH
bun=$(MISE_AUTO_INSTALL=false MISE_EXEC_AUTO_INSTALL=false MISE_LOCKFILE=0 mise --no-config --no-env --no-hooks which bun --tool bun@{BUN_VERSION})
case "$bun" in /*) test -f "$bun"; test -x "$bun"; test ! -L "$bun" ;; *) printf '%s\n' 'pinned Bun executable must be absolute' >&2; exit 1 ;; esac
export VELNOR_FIXTURE_ORIGIN_TOKEN="$fixture_origin_credential"
unset fixture_origin_credential
exec "$bun" "$fixture_workspace/scripts/provision-nonrust-tools-fixture.ts" "$RUNNER_TEMP/velnor-nonrust-tools-fixture"
"#,
        denied = denied.join(" ")
    )
}

//! Resolve `[stacks.dbt]` into one job policy per project.

use velnor_actions_contract::config::DbtService;
use velnor_actions_contract::{JobTimeout, ServiceContainer, VelnorConfig};
use velnor_actions_dbt::{job_id, job_name};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::verification_jobs::{StackJobKind, StackJobPolicy};

use super::dbt_steps::project_steps;
use crate::OrchestratorError;
use crate::config::CONFIG_REL;
use crate::discover::Discovery;

/// Postgres major the service catalog offers for dbt warehouse stand-ins.
const POSTGRES_MAJOR: u8 = 18;

/// Policies for every project, sorted by job id.
pub(crate) fn policies(
    config: &VelnorConfig,
    discovery: &Discovery,
    catalog: &ToolCatalog,
) -> Result<Vec<StackJobPolicy>, OrchestratorError> {
    let Some(dbt) = &config.stacks.dbt else {
        return Ok(Vec::new());
    };
    let timeout = dbt
        .timeout_minutes
        .map_or(Ok(config.workflow.crate_timeout_minutes), u16::try_from)
        .map_err(|_| failure("dbt_timeout"))?;
    let timeout = JobTimeout::new(timeout)?;
    let setup = crate::pins::resolve_mise_setup_for_release_target(
        config,
        velnor_actions_contract::ReleaseTarget::LinuxX86_64,
    )?;
    let mut services = Vec::new();
    for service in &dbt.services {
        match service {
            DbtService::Postgres => {
                let container = ServiceContainer::postgres(POSTGRES_MAJOR).map_err(|err| {
                    OrchestratorError::Contract {
                        problem: err.to_string(),
                    }
                })?;
                check_postgres_env(&dbt.env, &container)?;
                services.push(container);
            }
        }
    }
    let mut out = Vec::new();
    for project in &discovery.dbt.projects {
        out.push(StackJobPolicy {
            kind: StackJobKind::Dbt,
            id: job_id(project),
            display_name: job_name(project),
            timeout,
            services: services.clone(),
            setup: setup.clone(),
            steps: project_steps(config, project, catalog)?,
        });
    }
    out.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(out)
}

/// Connection values the catalog Postgres service publishes (host port, credentials).
fn check_postgres_env(
    env: &std::collections::BTreeMap<String, String>,
    service: &ServiceContainer,
) -> Result<(), OrchestratorError> {
    let host_port = service
        .ports
        .first()
        .and_then(|port| port.split(':').next())
        .unwrap_or_default();
    let user = |key: &str| service.env.get(key).map_or("", String::as_str);
    for (key, expected) in [
        ("DBT_PORT", host_port),
        ("DBT_USER", user("POSTGRES_USER")),
        ("DBT_PASSWORD", user("POSTGRES_PASSWORD")),
        ("DBT_DBNAME", user("POSTGRES_DB")),
    ] {
        if env.get(key).is_some_and(|value| value != expected) {
            return Err(failure(&format!("dbt_env_service_mismatch:{key}")));
        }
    }
    Ok(())
}

fn failure(problem: &str) -> OrchestratorError {
    OrchestratorError::config(CONFIG_REL, "stacks.dbt", problem)
}

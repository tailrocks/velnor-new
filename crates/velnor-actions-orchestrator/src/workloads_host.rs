//! Host-qualified native workload vectors and toolchain identities.

use velnor_actions_contract::{ContractError, ProposedTask, Stack, digest_b3};
use velnor_actions_mise::ToolCatalog;

use crate::OrchestratorError;

use super::{catalog_for_configuration, identity_recipe, metadata, runner_for_task, tools};

/// Resolve a workflow runner label to the qualified native distribution host.
///
/// Native selectors never infer a host from the ambient process. The emitted
/// runner label is the only authority passed into the Mise catalog.
pub(crate) fn host_for_runner(
    label: &str,
) -> Result<velnor_actions_mise::catalog::qualification::DistributionHost, OrchestratorError> {
    let target = velnor_actions_contract::tool_target_for_runner_label(label).ok_or_else(|| {
        OrchestratorError::Contract {
            problem: format!("unsupported_target_for_runner:{label}"),
        }
    })?;
    match target {
        "x86_64-unknown-linux-gnu" => {
            Ok(velnor_actions_mise::catalog::qualification::DistributionHost::LinuxAmd64)
        }
        "aarch64-unknown-linux-gnu" => {
            Ok(velnor_actions_mise::catalog::qualification::DistributionHost::LinuxArm64)
        }
        "aarch64-apple-darwin" => {
            Ok(velnor_actions_mise::catalog::qualification::DistributionHost::MacosArm64)
        }
        _ => Err(OrchestratorError::Contract {
            problem: format!("unsupported_target_for_runner:{label}"),
        }),
    }
}

/// Isolated native fixed vector bound to the emitted runner host.
pub(crate) fn argv_for_runner(
    task: &ProposedTask,
    catalog: &ToolCatalog,
    label: &str,
) -> Result<Vec<String>, OrchestratorError> {
    require_workload(task)?;
    let failed = |error: velnor_actions_mise::MiseError| OrchestratorError::Contract {
        problem: error.to_string(),
    };
    let actual_label = runner_for_task(task, label);
    let host = host_for_runner(actual_label)?;
    let scoped_catalog = catalog_for_configuration(catalog, &task.configuration).map_err(failed)?;
    let request = velnor_actions_mise::catalog::workload::WorkloadExec::new(
        &task.identity.project_root,
        tools(&task.configuration)?,
        task.payload.clone(),
    )
    .map_err(failed)?;
    crate::utf8::strings_of(
        request
            .argv_for_host(&scoped_catalog, host)
            .map_err(failed)?,
    )
    .map_err(|problem| OrchestratorError::Contract { problem })
}

/// Exact native tool pins and execution policy bound to the emitted runner.
pub(crate) fn toolchain_id_for_runner(
    task: &ProposedTask,
    catalog: &ToolCatalog,
    label: &str,
) -> Result<String, ContractError> {
    require_workload(task)
        .map_err(|error| ContractError::identity("native_host", error.to_string()))?;
    let actual_label = runner_for_task(task, label);
    let host = host_for_runner(actual_label)
        .map_err(|error| ContractError::identity("native_host", error.to_string()))?;
    let scoped_catalog = catalog_for_configuration(catalog, &task.configuration)
        .map_err(|error| ContractError::identity("native_catalog", error.to_string()))?;
    let pins = scoped_catalog
        .native_tool_specs(host, &tools(&task.configuration)?)
        .map_err(|error| ContractError::identity("native_catalog", error.to_string()))?
        .join("\n");
    let fixture = metadata::fixture_identity(task)
        .map_err(|error| ContractError::identity("native_fixture", error.to_string()))?
        .unwrap_or_default();
    let desktop = identity_recipe::profile_identity(task)
        .map_err(|error| ContractError::identity("native_desktop_profile", error.to_string()))?
        .unwrap_or_default();
    Ok(digest_b3(
        format!(
            "native-validation-v1\n{}\n{pins}\n{fixture}\n{desktop}",
            task.configuration
        )
        .as_bytes(),
    ))
}

fn require_workload(task: &ProposedTask) -> Result<(), OrchestratorError> {
    if Stack::from_id(&task.stack_id) == Some(Stack::Workload) {
        Ok(())
    } else {
        Err(OrchestratorError::Contract {
            problem: "native_host_for_non_workload".to_owned(),
        })
    }
}

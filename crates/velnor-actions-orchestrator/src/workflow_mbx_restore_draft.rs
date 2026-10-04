//! Private source-only lowering; qualified owner acquisition stays outside fixture data.
use super::invalid;
use crate::OrchestratorError;
use std::collections::BTreeMap;
use velnor_actions_contract::{MbxExportDescriptor, Step, StepId, StepKind, ToolCacheDomain};
use velnor_actions_mise::{
    ToolCatalog, catalog::mbx_action_authority::QualifiedMbxAction,
    catalog::qualification::QualifiedDistribution,
};

struct RestoreSource<'a> {
    uses: String,
    path: &'a str,
    version: &'a str,
    binary_sha256: &'a str,
}

pub(super) fn restore_draft(
    descriptor: &MbxExportDescriptor,
    catalog: &ToolCatalog,
    action: QualifiedMbxAction,
    owner: &QualifiedDistribution,
) -> Result<Step, OrchestratorError> {
    let repository = action
        .source_repository()
        .strip_prefix("https://github.com/")
        .ok_or_else(|| invalid("action_repository_not_github"))?;
    let uses = velnor_actions_actionlint::PinnedActionRef::new(
        repository,
        None,
        action.source_commit(),
        action.release_version(),
    )?
    .uses_value();
    lower_restore(
        descriptor,
        catalog,
        RestoreSource {
            uses,
            path: owner.required_installed_binary_path()?,
            version: owner.version(),
            binary_sha256: owner.binary_sha256(),
        },
    )
}

fn lower_restore(
    descriptor: &MbxExportDescriptor,
    catalog: &ToolCatalog,
    source: RestoreSource<'_>,
) -> Result<Step, OrchestratorError> {
    let prefix = descriptor.cache_prefix()?;
    let mut env = crate::matrix_step::task_step_env(catalog, &BTreeMap::new(), true)?;
    env.insert(
        "MISE_DATA_DIR".to_owned(),
        ToolCacheDomain::Full.root().to_owned(),
    );
    Ok(Step {
        id: Some(StepId::new(crate::mbx_export::RESTORE_ID)?),
        name: "Draft native MBX restore (not activated)".to_owned(),
        condition: None,
        kind: StepKind::Action {
            uses: source.uses,
            env,
            with: BTreeMap::from([
                ("backend".to_owned(), "github".to_owned()),
                ("github-cache-mode".to_owned(), "objects".to_owned()),
                (
                    "mbx-path".to_owned(),
                    format!("{}/{}", ToolCacheDomain::Full.root(), source.path),
                ),
                ("expected-version".to_owned(), source.version.to_owned()),
                (
                    "expected-binary-sha256".to_owned(),
                    source.binary_sha256.to_owned(),
                ),
                ("comparison-state".to_owned(), descriptor.comparison_path()?),
                ("export-group".to_owned(), descriptor.export_group()?),
                (
                    "cache-generation".to_owned(),
                    "velnor-mbx-native-v4".to_owned(),
                ),
                (
                    "cache-key".to_owned(),
                    format!(
                        "{prefix}lookup-${{{{ github.run_id }}}}-${{{{ github.run_attempt }}}}"
                    ),
                ),
                ("restore-keys".to_owned(), format!("{prefix}snapshot-")),
                ("toolchain".to_owned(), catalog.rustup_toolchain()),
            ]),
        },
    })
}

#[cfg(test)]
#[path = "workflow_mbx_restore_draft_tests.rs"]
mod tests;

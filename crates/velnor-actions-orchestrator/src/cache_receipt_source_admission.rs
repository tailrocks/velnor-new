//! Reconstruct every executable byte from its sole source owner before signing.
use super::{OrchestratorError, invalid};
use velnor_actions_contract::{CacheSnapshotDomain, CompiledSourceHelper, SourceBoundOperation};
use velnor_actions_workflow_renderer::cache_producer_workflow::CacheProducerRecipe;

pub(super) fn validate(recipe: &CacheProducerRecipe) -> Result<(), OrchestratorError> {
    validate_records(recipe, None, None)
}

pub(super) fn validate_with_native(
    recipe: &CacheProducerRecipe,
    owner: &super::native::NativeReceiptRecipe,
) -> Result<(), OrchestratorError> {
    super::native::validate(recipe, owner)?;
    validate_records(recipe, Some(owner), None)
}

pub(super) fn validate_with_mbx(
    recipe: &CacheProducerRecipe,
    owner: &crate::mbx_producer::DraftMbxProducer,
) -> Result<(), OrchestratorError> {
    owner.validate_recipe(recipe)?;
    validate_records(recipe, None, Some(owner))
}

fn validate_records(
    recipe: &CacheProducerRecipe,
    native: Option<&super::native::NativeReceiptRecipe>,
    mbx: Option<&crate::mbx_producer::DraftMbxProducer>,
) -> Result<(), OrchestratorError> {
    for actual in recipe.source_helpers() {
        let expected = reconstruct(recipe, actual, native, mbx)?;
        if expected != *actual {
            return Err(invalid("producer_helper_source_owner_mismatch"));
        }
    }
    Ok(())
}

fn reconstruct(
    recipe: &CacheProducerRecipe,
    actual: &CompiledSourceHelper,
    native: Option<&super::native::NativeReceiptRecipe>,
    mbx: Option<&crate::mbx_producer::DraftMbxProducer>,
) -> Result<CompiledSourceHelper, OrchestratorError> {
    let invocation = actual.invocation();
    let environment = actual.environment();
    let version = recipe.generator_version();
    match invocation.descriptor().operation() {
        SourceBoundOperation::MiseBootstrap => {
            velnor_actions_mise::catalog::mise_acquisition::record_for_invocation(
                invocation,
                environment,
                version,
            )
            .map_err(owner_error)
        }
        SourceBoundOperation::MiseToolPrepare
        | SourceBoundOperation::RustPrepareRootLinux
        | SourceBoundOperation::RustPrepareDesktopMac
        | SourceBoundOperation::RustPrepareDesktopSourceMac => {
            velnor_actions_mise::catalog::tool_prepare::record_for_invocation(
                invocation,
                environment,
                version,
            )
            .map_err(owner_error)
        }
        SourceBoundOperation::CacheSnapshot => snapshot(actual, version),
        SourceBoundOperation::RustSourceProducer => super::rust::reconstruct(recipe, actual),
        SourceBoundOperation::ToolProducerReport => {
            let metadata = recipe
                .original()
                .tool_producer
                .as_ref()
                .ok_or_else(|| invalid("producer_report_foreign_role"))?;
            Ok(velnor_actions_workflow_renderer::cache_p08::report_record(
                metadata, version,
            )?)
        }
        SourceBoundOperation::SourceProducerReport => {
            let metadata = recipe
                .original()
                .source_producer
                .as_ref()
                .ok_or_else(|| invalid("producer_report_foreign_role"))?;
            crate::workloads::cache::source_report::record(metadata, version)
        }
        SourceBoundOperation::NpmPublicSourceProducer
        | SourceBoundOperation::BunSourceProducer
        | SourceBoundOperation::TofuProviderExport => native_record(actual, native),
        SourceBoundOperation::MbxProducerPrepare
        | SourceBoundOperation::MbxArtifactAdmission
        | SourceBoundOperation::MbxBundleVerify
        | SourceBoundOperation::MbxProducerReport => mbx_record(actual, mbx),
        _ => Err(invalid("producer_source_gate_unqualified")),
    }
}

fn mbx_record(
    actual: &CompiledSourceHelper,
    owner: Option<&crate::mbx_producer::DraftMbxProducer>,
) -> Result<CompiledSourceHelper, OrchestratorError> {
    // Complete job, role, permissions and records were freshly reconstructed above.
    let owner = owner.ok_or_else(|| invalid("mbx_producer_source_gate_unqualified"))?;
    let records = owner
        .source_helpers()
        .iter()
        .filter(|expected| *expected == actual)
        .collect::<Vec<_>>();
    let [expected] = records.as_slice() else {
        return Err(invalid("mbx_producer_source_owner_mismatch"));
    };
    Ok((*expected).clone())
}

fn native_record(
    actual: &CompiledSourceHelper,
    owner: Option<&super::native::NativeReceiptRecipe>,
) -> Result<CompiledSourceHelper, OrchestratorError> {
    // The capsule's complete job and records were freshly regenerated above.
    let owner = owner.ok_or_else(|| invalid("native_producer_source_gate_unqualified"))?;
    let records = owner
        .source_helpers()
        .iter()
        .filter(|expected| *expected == actual)
        .collect::<Vec<_>>();
    let [expected] = records.as_slice() else {
        return Err(invalid("native_producer_source_owner_mismatch"));
    };
    Ok((*expected).clone())
}

fn snapshot(
    actual: &CompiledSourceHelper,
    version: &str,
) -> Result<CompiledSourceHelper, OrchestratorError> {
    let args = actual.invocation().args();
    let [domain, phase] = args else {
        return Err(invalid("snapshot_source_gate_unqualified"));
    };
    let domain = CacheSnapshotDomain::ALL
        .into_iter()
        .find(|candidate| candidate.name() == domain)
        .ok_or_else(|| invalid("snapshot_source_gate_unqualified"))?;
    let before = match phase.as_str() {
        "before" => true,
        "after" => false,
        _ => return Err(invalid("snapshot_source_gate_unqualified")),
    };
    crate::workloads::cache::source_snapshot::record(domain, before, version)
}

fn owner_error(error: velnor_actions_mise::MiseError) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use velnor_actions_contract::{HelperInvocation, SourceBoundHelper};

    #[test]
    fn correctly_tagged_arbitrary_snapshot_body_cannot_pass_owner_admission() {
        let domain = CacheSnapshotDomain::PlanningTools;
        let source = velnor_actions_contract::generated_source(
            env!("CARGO_PKG_VERSION"),
            "echo arbitrary caller computation\n",
        )
        .expect("marker");
        let operation = SourceBoundOperation::CacheSnapshot;
        let descriptor = SourceBoundHelper::compiled(
            operation,
            operation.path(),
            &velnor_actions_contract::compiled_source_sha256(source.as_bytes()),
        )
        .expect("descriptor");
        let invocation = HelperInvocation::compiled(
            descriptor,
            vec![domain.name().into(), "before".into()],
            Vec::new(),
        )
        .expect("invocation");
        let actual = CompiledSourceHelper::compiled(invocation, source)
            .expect("record")
            .with_environment(domain.environment(true));
        let expected = snapshot(&actual, env!("CARGO_PKG_VERSION")).expect("owner source");
        assert_ne!(actual, expected);
        actual
            .validate_binding()
            .expect("generic shape and digest still valid");
    }

    #[test]
    fn exact_owner_snapshot_reconstructs_complete_source_bytes() {
        let actual = crate::workloads::cache::source_snapshot::record(
            CacheSnapshotDomain::PlanningTools,
            true,
            env!("CARGO_PKG_VERSION"),
        )
        .expect("owner source");
        assert_eq!(
            snapshot(&actual, env!("CARGO_PKG_VERSION")).expect("reconstruction"),
            actual
        );
    }
}

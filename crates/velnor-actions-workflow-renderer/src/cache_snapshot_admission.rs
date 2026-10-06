//! Neutral admission of snapshots supplied by their compiled source owner.
use crate::RenderError;
use velnor_actions_contract::{
    CacheSnapshotDomain, CompiledSourceHelper, SourceBoundOperation, Step, StepId,
};

/// Bind the exact qualified observer to a closed domain, phase and evidence ID.
/// # Errors
/// Rejects wrong operations, arguments, roots, restore references or environment.
pub(crate) fn snapshot_step(
    domain: CacheSnapshotDomain,
    before: bool,
    id: &StepId,
    record: &CompiledSourceHelper,
) -> Result<Step, RenderError> {
    let phase = phase(before);
    let invocation = record.invocation();
    if invocation.descriptor().operation() != SourceBoundOperation::CacheSnapshot
        || invocation.args() != [domain.name(), phase]
        || !invocation.installed_selectors().is_empty()
        || record.environment() != &domain.environment(before)
    {
        return Err(invalid("snapshot_owner_binding_changed"));
    }
    let mut step = crate::source_helper::source_helper_step(
        &format!("Measure {} snapshot {phase}", domain.name()),
        record,
        record.environment().clone(),
    )?;
    step.id = Some(id.clone());
    Ok(step)
}

/// Select a unique compiled owner record from the enclosing render registry.
/// # Errors
/// Rejects missing, ambiguous or detached snapshot authority.
pub(crate) fn registered_snapshot_step(
    domain: CacheSnapshotDomain,
    before: bool,
    id: &StepId,
    records: &[CompiledSourceHelper],
) -> Result<Step, RenderError> {
    let candidates: Vec<_> = records
        .iter()
        .filter(|record| {
            record.invocation().descriptor().operation() == SourceBoundOperation::CacheSnapshot
                && record.invocation().args() == [domain.name(), phase(before)]
        })
        .collect();
    let [record] = candidates.as_slice() else {
        return Err(invalid("snapshot_owner_missing_or_ambiguous"));
    };
    snapshot_step(domain, before, id, record)
}

const fn phase(before: bool) -> &'static str {
    if before { "before" } else { "after" }
}

fn invalid(reason: &str) -> RenderError {
    RenderError::InvalidWorkflow(reason.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use velnor_actions_contract::{HelperInvocation, SourceBoundHelper};

    fn record(args: Vec<String>, env: BTreeMap<String, String>) -> CompiledSourceHelper {
        let source = crate::marker::with_marker("0.1.0", "exit 0\n").expect("source");
        let operation = SourceBoundOperation::CacheSnapshot;
        let descriptor = SourceBoundHelper::compiled(
            operation,
            operation.path(),
            &velnor_actions_contract::compiled_source_sha256(source.as_bytes()),
        )
        .expect("descriptor");
        let invocation =
            HelperInvocation::compiled(descriptor, args, Vec::new()).expect("invocation");
        CompiledSourceHelper::compiled(invocation, source)
            .expect("record")
            .with_environment(env)
    }

    #[test]
    fn every_domain_and_phase_requires_exact_registered_owner() {
        let id = StepId::new("observer").expect("id");
        for domain in CacheSnapshotDomain::ALL {
            for before in [true, false] {
                let owner = record(
                    vec![domain.name().to_owned(), phase(before).to_owned()],
                    domain.environment(before),
                );
                let expected = snapshot_step(domain, before, &id, &owner).expect("canonical");
                assert_eq!(
                    registered_snapshot_step(domain, before, &id, &[owner]).expect("registered"),
                    expected
                );
                assert!(registered_snapshot_step(domain, before, &id, &[]).is_err());
            }
        }
    }

    #[test]
    fn altered_roots_phase_controls_or_restore_bindings_are_rejected() {
        let domain = CacheSnapshotDomain::Tools;
        let id = StepId::new("observer").expect("id");
        for key in [
            "VELNOR_SNAPSHOT_LAYER",
            "VELNOR_SNAPSHOT_PHASE",
            "VELNOR_SNAPSHOT_ROOTS",
            "VELNOR_SNAPSHOT_OUTPUT",
            "VELNOR_SNAPSHOT_RESTORED",
        ] {
            let mut env = domain.environment(true);
            env.insert(key.to_owned(), "injected".to_owned());
            let owner = record(vec![domain.name().to_owned(), "before".to_owned()], env);
            assert!(snapshot_step(domain, true, &id, &owner).is_err(), "{key}");
            assert!(
                registered_snapshot_step(domain, true, &id, &[owner]).is_err(),
                "{key}"
            );
        }
        for args in [
            vec!["sources".to_owned(), "before".to_owned()],
            vec!["tools".to_owned(), "after".to_owned()],
            vec!["tools".to_owned(), "before".to_owned(), "extra".to_owned()],
        ] {
            let owner = record(args, domain.environment(true));
            assert!(snapshot_step(domain, true, &id, &owner).is_err());
        }
    }

    #[test]
    fn ambiguous_or_detached_authority_is_rejected() {
        let domain = CacheSnapshotDomain::PlanningTools;
        let id = StepId::new("observer").expect("id");
        let owner = record(
            vec![domain.name().to_owned(), "before".to_owned()],
            domain.environment(true),
        );
        assert!(registered_snapshot_step(domain, true, &id, &[owner.clone(), owner]).is_err());
    }
}

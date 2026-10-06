//! Caller-approved bootstrap identity; serialization never resolves tool policy.
use velnor_actions_contract::{
    CompiledSourceHelper, SourceBoundOperation, StepKind, ToolCacheDomain,
};

use crate::{MiseSetup, RenderError, release_jobs::ReleaseJobSpec};

/// Frozen checkout and source-owner acquisition authority for a release family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseBootstrapApproval {
    /// Exact approved checkout action reference.
    pub checkout_uses: String,
    /// Owner-qualified acquisition records keyed by domain and actual runner.
    pub mise: MiseSetup,
}

impl ReleaseBootstrapApproval {
    /// Validate neutral authority shape; the generator factory owns qualification.
    /// # Errors
    /// Rejects malformed checkout refs or source acquisition records.
    pub fn validate(&self) -> Result<(), RenderError> {
        crate::steps::validate_uses(&self.checkout_uses)?;
        if !self.checkout_uses.starts_with("actions/checkout@") {
            return Err(invalid("release_checkout_approval"));
        }
        self.mise.validate()
    }

    /// Require one exact applicable bootstrap and no duplicate operation descriptors.
    /// # Errors
    /// Rejects missing, substituted or conflicting frozen registry records.
    pub fn check_registry(
        &self,
        records: &[CompiledSourceHelper],
        runs_on: &str,
    ) -> Result<(), RenderError> {
        self.validate()?;
        let mut domains = Vec::new();
        for (index, record) in records.iter().enumerate() {
            let descriptor = record.invocation().descriptor();
            if records[..index].iter().any(|previous| {
                let prior = previous.invocation().descriptor();
                (prior.operation() == descriptor.operation() || prior.path() == descriptor.path())
                    && !distinct_scoped_tools(previous, record)
            }) {
                return Err(invalid("release_helper_registry_duplicate"));
            }
            if descriptor.operation() == SourceBoundOperation::MiseBootstrap {
                let domain = record_domain(record)
                    .ok_or_else(|| invalid("release_bootstrap_registry_authority"))?;
                if record != &self.mise.bootstrap(domain, runs_on)?.helper {
                    return Err(invalid("release_bootstrap_registry_authority"));
                }
                domains.push(domain);
            }
        }
        if !domains.contains(&ToolCacheDomain::Full) || domains.len() > 2 {
            return Err(invalid("release_bootstrap_registry_missing"));
        }
        Ok(())
    }

    /// Bind one job to its exact registry record and bootstrap execution order.
    /// # Errors
    /// Rejects substituted, missing, duplicate, late or foreign-host acquisition.
    pub fn check_job(&self, id: &str, job: &ReleaseJobSpec) -> Result<(), RenderError> {
        self.validate()?;
        if job.role == crate::release_jobs::ReleaseRole::SourceSnapshotForge {
            check_source_tool_domains(id, job)?;
        }
        let domains: &[ToolCacheDomain] =
            if job.role == crate::release_jobs::ReleaseRole::SourceSnapshotForge {
                &[ToolCacheDomain::Full, ToolCacheDomain::Planning]
            } else {
                &[ToolCacheDomain::Full]
            };
        let count = job
            .steps
            .iter()
            .filter(|step| {
                matches!(&step.kind, StepKind::SourceBoundHelper { invocation, .. }
                if invocation.descriptor().operation() == SourceBoundOperation::MiseBootstrap)
            })
            .count();
        if count != domains.len() {
            let reason = if count == 0 {
                "release_bootstrap_missing"
            } else {
                "release_bootstrap_authority"
            };
            return Err(invalid(&format!("{reason}:{id}")));
        }
        let mut acquired = 0;
        for step in &job.steps {
            match &step.kind {
                StepKind::Action { uses, .. } if uses.starts_with("actions/checkout@") => {
                    if uses != &self.checkout_uses {
                        return Err(invalid(&format!("release_checkout_authority:{id}")));
                    }
                }
                StepKind::SourceBoundHelper { invocation, env }
                    if invocation.descriptor().operation()
                        == SourceBoundOperation::MiseBootstrap =>
                {
                    let domain = domains
                        .get(acquired)
                        .ok_or_else(|| invalid(&format!("release_bootstrap_authority:{id}")))?;
                    let expected = self.mise.bootstrap(*domain, &job.runs_on)?;
                    if invocation != expected.helper.invocation()
                        || env != expected.helper.environment()
                        || step.condition.is_some()
                    {
                        return Err(invalid(&format!("release_bootstrap_authority:{id}")));
                    }
                    acquired += 1;
                }
                StepKind::Shell { .. } | StepKind::SourceBoundHelper { .. }
                    if acquired != domains.len() =>
                {
                    return Err(invalid(&format!("release_bootstrap_order:{id}")));
                }
                StepKind::Shell { .. } | StepKind::SourceBoundHelper { .. } => {}
                _ => {}
            }
        }
        if acquired != domains.len() {
            return Err(invalid(&format!("release_bootstrap_missing:{id}")));
        }
        Ok(())
    }
}

fn check_source_tool_domains(id: &str, job: &ReleaseJobSpec) -> Result<(), RenderError> {
    use SourceBoundOperation::{MiseBootstrap, MiseToolPrepare};
    let actual: Vec<_> = job
        .steps
        .iter()
        .filter_map(|step| {
            let StepKind::SourceBoundHelper { invocation, env } = &step.kind else {
                return None;
            };
            let operation = invocation.descriptor().operation();
            matches!(operation, MiseBootstrap | MiseToolPrepare)
                .then(|| (operation, env.get("MISE_DATA_DIR").map(String::as_str)))
        })
        .collect();
    let expected = [
        (MiseBootstrap, Some(ToolCacheDomain::Full.root())),
        (MiseBootstrap, Some(ToolCacheDomain::Planning.root())),
        (MiseToolPrepare, Some(ToolCacheDomain::Full.root())),
        (MiseToolPrepare, Some(ToolCacheDomain::Planning.root())),
    ];
    if actual != expected {
        return Err(invalid(&format!("release_source_tool_domains:{id}")));
    }
    Ok(())
}

fn record_domain(record: &CompiledSourceHelper) -> Option<ToolCacheDomain> {
    [ToolCacheDomain::Full, ToolCacheDomain::Planning]
        .into_iter()
        .find(|domain| {
            record
                .environment()
                .get("MISE_DATA_DIR")
                .map(String::as_str)
                == Some(domain.root())
        })
}

fn distinct_scoped_tools(left: &CompiledSourceHelper, right: &CompiledSourceHelper) -> bool {
    let prior = left.invocation().descriptor();
    let next = right.invocation().descriptor();
    prior.operation() == next.operation()
        && prior.path() == next.path()
        && left.source() == right.source()
        && matches!(
            prior.operation(),
            SourceBoundOperation::MiseBootstrap | SourceBoundOperation::MiseToolPrepare
        )
        && matches!((record_domain(left), record_domain(right)), (Some(left), Some(right)) if left != right)
}

fn invalid(reason: &str) -> RenderError {
    RenderError::InvalidWorkflow(reason.to_owned())
}

//! Private compiler handoff from source-owned proposals to obligation rendering.

use crate::{
    OrchestratorError, helper_obligation_binding::ProposalHelperBinding, internal::internal,
};
use std::collections::BTreeMap;
use velnor_actions_contract::{CompiledSourceHelper, CrateObligation, HelperObligationDescriptor};

#[derive(Default)]
pub(crate) struct SourceBindings {
    records: BTreeMap<String, SourceBinding>,
}

struct SourceBinding {
    obligation: CrateObligation,
    execution: ExecutionRecipe,
}

pub(crate) enum ExecutionRecipe {
    NativeHelper {
        descriptor: Box<HelperObligationDescriptor>,
        record: Box<CompiledSourceHelper>,
    },
    AlreadyReportedCompiler(Box<crate::rust_report_wrapper::RustReportWrapper>),
}

impl SourceBindings {
    pub(crate) fn insert_source_binding(
        &mut self,
        obligation: &CrateObligation,
        binding: ProposalHelperBinding,
    ) -> Result<(), OrchestratorError> {
        binding.record.validate_binding()?;
        if binding.descriptor
            != HelperObligationDescriptor::from_compiled(&binding.record, &obligation.matrix_key)?
        {
            return Err(internal("helper_obligation_descriptor_mismatch"));
        }
        if self.records.contains_key(&obligation.task_id) {
            return Err(internal("helper_obligation_duplicate_binding"));
        }
        self.records.insert(
            obligation.task_id.clone(),
            SourceBinding {
                obligation: obligation.clone(),
                execution: ExecutionRecipe::NativeHelper {
                    descriptor: Box::new(binding.descriptor),
                    record: Box::new(binding.record),
                },
            },
        );
        Ok(())
    }

    pub(crate) fn insert_compiler_recipe(
        &mut self,
        obligation: &CrateObligation,
        recipe: crate::rust_report_wrapper::RustReportWrapper,
    ) -> Result<(), OrchestratorError> {
        if self.records.contains_key(&obligation.task_id) {
            return Err(internal("helper_obligation_duplicate_binding"));
        }
        self.records.insert(
            obligation.task_id.clone(),
            SourceBinding {
                obligation: obligation.clone(),
                execution: ExecutionRecipe::AlreadyReportedCompiler(Box::new(recipe)),
            },
        );
        Ok(())
    }

    pub(crate) fn execution_for(
        &self,
        obligation: &CrateObligation,
    ) -> Result<Option<&ExecutionRecipe>, OrchestratorError> {
        let Some(binding) = self.records.get(&obligation.task_id) else {
            if crate::helper_obligation_binding::requires_helper_task(&obligation.task_id)
                || crate::rust_report_wrapper::requires_compiler_task(&obligation.task_id)
            {
                return Err(internal("helper_obligation_source_binding_missing"));
            }
            return Ok(None);
        };
        if binding.obligation != *obligation {
            return Err(internal("helper_obligation_source_binding_mismatch"));
        }
        if let ExecutionRecipe::NativeHelper { descriptor, record } = &binding.execution {
            if descriptor.as_ref()
                != &HelperObligationDescriptor::from_compiled(record, &obligation.matrix_key)?
            {
                return Err(internal("helper_obligation_source_binding_mismatch"));
            }
            record.validate_binding()?;
        }
        Ok(Some(&binding.execution))
    }

    pub(crate) fn validate_coverage(
        &self,
        obligations: &[CrateObligation],
    ) -> Result<(), OrchestratorError> {
        let mut seen = std::collections::BTreeSet::new();
        for obligation in obligations {
            if !seen.insert(obligation.task_id.as_str()) {
                return Err(internal("helper_obligation_duplicate_task"));
            }
            self.execution_for(obligation)?;
        }
        if self
            .records
            .keys()
            .any(|task| !seen.contains(task.as_str()))
        {
            return Err(internal("helper_obligation_unused_binding"));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "crate_obligation_helpers_tests.rs"]
mod tests;

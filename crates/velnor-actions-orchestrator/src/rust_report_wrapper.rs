//! Private Rust compiler recipe, followed by an exact ordinary task-report frame.

use std::collections::BTreeMap;

use velnor_actions_contract::{
    CompiledRustReportRecipe, CompiledSourceHelper, CompilerDriver, CrateObligation, ProposedTask,
    RustCompilerOperation, RustCompilerTools, RustReportFrame, Stack,
};
use velnor_actions_mise::{RouteDriver, ToolCatalog};

use crate::{OrchestratorError, internal::internal};

/// Closed Rust operations which execute the selected compiler route.
pub(crate) fn requires_compiler(task: &ProposedTask) -> bool {
    task.stack_id == Stack::Rust.id() && requires_compiler_task(&task.task_id)
}

#[cfg(test)]
#[path = "rust_report_wrapper_tests.rs"]
mod tests;

/// Missing-sidecar denial for the closed compiler task identity inventory.
pub(crate) fn requires_compiler_task(task_id: &str) -> bool {
    crate::extension_schemas::task_stack_segment(task_id) == Some(Stack::Rust.id())
        && matches!(
            crate::extension_schemas::task_kind_segment(task_id),
            Some("clippy" | "test" | "nextest" | "doctest" | "doc" | "build")
        )
}

/// Original adapter proposal and compiler identity; never reconstructed from wire data.
pub(crate) struct RustReportWrapper {
    task: ProposedTask,
    argv: Vec<String>,
    toolchain: String,
    digest: String,
    driver: RouteDriver,
}

impl RustReportWrapper {
    /// Compile the adapter's fixed execution recipe before report-frame binding.
    pub(crate) fn from_proposal(
        task: &ProposedTask,
        catalog: &ToolCatalog,
        label: &str,
    ) -> Result<Option<Self>, OrchestratorError> {
        if !requires_compiler(task) {
            return Ok(None);
        }
        task.validate()?;
        let base = velnor_actions_contract::split_shard_suffix(&task.task_id)
            .map_or(task.task_id.as_str(), |(base, _, _)| base);
        if crate::extension_schemas::task_kind_segment(&task.task_id)
            != Some(task.task_kind.as_str())
            || base.rsplit('/').next() != Some(task.configuration.as_str())
        {
            return Err(internal("rust_report_wrapper_proposal_identity"));
        }
        let driver = RouteDriver::from_compile_driver(&task.identity.compile_driver)
            .ok_or_else(|| internal("rust_report_wrapper_unknown_driver"))?;
        velnor_actions_rust::TestRunner::parse(&task.identity.test_runner)?;
        let argv = crate::vectors::task_argv_for_runner(task, catalog, label)?;
        let toolchain = crate::internal_plan::toolchain_id_for_runner(task, catalog, label)?;
        let digest = crate::internal::plan_obligation::task_digest(
            &task.task_id,
            &argv,
            &toolchain,
            None,
            None,
        )?;
        Ok(Some(Self {
            task: task.clone(),
            argv,
            toolchain,
            digest,
            driver,
        }))
    }

    pub(crate) fn argv(&self) -> &[String] {
        &self.argv
    }

    pub(crate) fn task_digest(&self) -> &str {
        &self.digest
    }

    /// Bind report data after the original argv/toolchain digest has been fixed.
    ///
    /// This record is a private rendering sidecar, never a helper descriptor in
    /// the task-digest preimage. The complete emitted source and environment
    /// remain bound by the compiled record's equality checks.
    pub(crate) fn bind_frame(
        &self,
        obligation: &CrateObligation,
        catalog: &ToolCatalog,
        downstream: &[String],
        matrix_cap: Option<u32>,
    ) -> Result<CompiledSourceHelper, OrchestratorError> {
        self.validate_obligation(obligation)?;
        let environment = self.environment(obligation, catalog, downstream, matrix_cap)?;
        let recipe = self.compiled_recipe(catalog)?;
        let frame = RustReportFrame::compiled(
            env!("CARGO_PKG_VERSION"),
            &self.task.task_id,
            &obligation.matrix_key,
        )?;
        CompiledSourceHelper::rust_report_wrapper(recipe, frame, environment)
            .map_err(OrchestratorError::from)
    }

    fn compiled_recipe(
        &self,
        catalog: &ToolCatalog,
    ) -> Result<CompiledRustReportRecipe, OrchestratorError> {
        let (driver, mbx) = match self.driver {
            RouteDriver::Cargo => (CompilerDriver::Cargo, None),
            RouteDriver::Mbx => (
                CompilerDriver::Mbx,
                Some(catalog.tool_spec(velnor_actions_mise::PinnedTool::MrBoxington)?),
            ),
        };
        let operation = match self.task.task_kind.as_str() {
            "clippy" => RustCompilerOperation::Clippy,
            "test" => RustCompilerOperation::Test,
            "nextest" => RustCompilerOperation::Nextest,
            "doctest" => RustCompilerOperation::Doctest,
            "doc" => RustCompilerOperation::Doc,
            "build" => RustCompilerOperation::Build,
            _ => return Err(internal("rust_report_wrapper_unknown_operation")),
        };
        let nextest = velnor_actions_rust::tool_needs(
            &self.task.identity.compile_driver,
            &self.task.identity.test_runner,
        )
        .nextest
        .then(|| catalog.tool_spec(velnor_actions_mise::PinnedTool::Nextest))
        .transpose()?;
        let tools = RustCompilerTools {
            rust: catalog.tool_spec(velnor_actions_mise::PinnedTool::Rust)?,
            mbx,
            nextest,
        };
        let payload = self
            .task
            .payload
            .iter()
            .map(|value| {
                value
                    .to_str()
                    .map(str::to_owned)
                    .ok_or_else(|| internal("rust_report_wrapper_payload_utf8"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let recipe = CompiledRustReportRecipe::compiled(
            driver,
            operation,
            tools,
            payload,
            self.toolchain.clone(),
            self.digest.clone(),
        )?;
        if recipe.compiler_argv() != self.argv {
            return Err(internal("rust_report_wrapper_canonical_vector"));
        }
        Ok(recipe)
    }

    fn validate_obligation(&self, obligation: &CrateObligation) -> Result<(), OrchestratorError> {
        let matrix_id = velnor_actions_contract::matrix_id_for_task_group(
            Stack::Rust.id(),
            &self.task.task_id,
        )?;
        let matrix_key = velnor_actions_contract::matrix_key_for_id(&matrix_id)?;
        if obligation.task_id != self.task.task_id
            || obligation.kind != self.task.task_kind
            || obligation.step_name
                != crate::matrix_step::step_name_for(&self.task.task_kind, &self.task.task_id)
            || obligation.run != self.argv
            || obligation.task_digest != self.digest
            || obligation.matrix_key != matrix_key
        {
            return Err(internal("rust_report_wrapper_obligation_mismatch"));
        }
        Ok(())
    }

    fn environment(
        &self,
        obligation: &CrateObligation,
        catalog: &ToolCatalog,
        downstream: &[String],
        matrix_cap: Option<u32>,
    ) -> Result<BTreeMap<String, String>, OrchestratorError> {
        let matrix_id = velnor_actions_contract::matrix_id_for_task_group(
            Stack::Rust.id(),
            &self.task.task_id,
        )?;
        let mut identity = crate::matrix_step::obligation_identity_env(
            &self.task.task_id,
            &self.digest,
            &matrix_id,
            &obligation.matrix_key,
            matrix_cap,
        );
        if !downstream.is_empty() {
            identity.insert(
                crate::task_report::DOWNSTREAM_IDS_ENV.to_owned(),
                downstream.join(","),
            );
        }
        for (key, value) in velnor_actions_rust::payload_env_for_kind(&self.task.task_kind) {
            identity.insert(
                key.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            );
        }
        identity.insert(
            crate::rust_report_preexec::FRAME_ARGV_ENV.to_owned(),
            serde_json::to_string(&self.argv)
                .map_err(|error| internal(&format!("rust_report_wrapper_frame_json:{error}")))?,
        );
        identity.insert(
            crate::rust_report_preexec::FRAME_TOOLCHAIN_ENV.to_owned(),
            self.toolchain.clone(),
        );
        crate::matrix_step::check_identity_env_contract(&identity, &self.task.task_id)?;
        crate::matrix_step::task_step_env(catalog, &identity, true)
    }
}

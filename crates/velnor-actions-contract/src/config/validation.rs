use crate::errors::ContractError;

use super::{
    DEFAULT_SUITE_WORKFLOW_SLUG, VelnorConfig, check_receipt_budget, validate_images,
    validate_qualified_tools, validate_suites_with_dispatch,
};

impl VelnorConfig {
    /// Schema 1: hosted-only generation. Schema 2 is routing.
    pub const SCHEMA: u32 = 1;
    /// Routing schema.
    pub const SCHEMA_V2: u32 = 2;
    /// Stack IDs registered in V1.
    pub const REGISTERED_STACKS: &'static [&'static str] = &["mise", "native", "rust", "tofu"];

    /// Validate every field; failures name file, key path, and problem.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        self.check_schema(file)?;
        self.workflow.validate(file)?;
        self.resources.validate(file)?;
        self.test_sharding.validate(file)?;
        self.stacks.validate(file)?;
        self.github.validate(file)?;
        self.tools.validate(file)?;
        if let Some(dco) = &self.dco {
            dco.validate(file)?;
        }
        self.check_toolchain_consistency(file)?;
        let suites = self.stacks.rust.as_ref().map_or(&[][..], |r| &r.suites);
        validate_suites_with_dispatch(suites, &self.workflow.dispatch, file)?;
        for suite in suites {
            let slug = suite
                .workflow
                .as_deref()
                .unwrap_or(DEFAULT_SUITE_WORKFLOW_SLUG);
            let path = format!(".github/workflows/{slug}.yml");
            if self.workflow.preserve_workflows.contains(&path) {
                return Err(ContractError::config(
                    file,
                    "workflow.preserve_workflows",
                    format!("preserve_workflow_generator_owned:{path}"),
                ));
            }
        }
        super::mise_task_suites::validate_rust_suite_slug_collisions(
            suites,
            &self.workflow.task_suites,
            file,
        )?;
        self.check_binary_release_authority(file)?;
        if let Some(apply) = &self.workflow.tofu_apply {
            let roots = self.stacks.tofu.as_ref().ok_or_else(|| {
                ContractError::config(
                    file,
                    "workflow.tofu_apply.root",
                    "requires_stacks_tofu_roots",
                )
            })?;
            if !roots.roots.contains(&apply.root) {
                return Err(ContractError::config(
                    file,
                    "workflow.tofu_apply.root",
                    "must_match_declared_tofu_root",
                ));
            }
        }
        self.distribution.validate(file)?;
        super::job_authority::validate_job_authority_bindings(&self.job_authority, file)?;
        if let Some(private_sources) = &self.private_sources {
            private_sources.validate(file)?;
        }
        if let Some(upgrades) = &self.upgrades {
            upgrades.validate(file)?;
        }
        self.publish.validate(file)?;
        self.check_distribution_source(file)?;
        self.check_binary_release_tools(file)?;
        self.check_crate_build_jobs(file)?;
        self.discovery.validate(file)?;
        self.actions.validate(file)?;
        validate_qualified_tools(&self.qualified_tools, file)?;
        self.check_checks(file)?;
        self.check_native_checks(file)?;
        validate_images(&self.images, file)?;
        self.check_shard_budgets(file)?;
        Ok(())
    }

    fn check_checks(&self, file: &str) -> Result<(), ContractError> {
        let mut check_ids = std::collections::BTreeSet::new();
        for (index, check) in self.checks.iter().enumerate() {
            check.validate(file, &format!("checks[{index}]"))?;
            for tool_id in &check.tools {
                if !self.qualified_tools.iter().any(|tool| &tool.id == tool_id) {
                    return Err(ContractError::config(
                        file,
                        format!("checks[{index}].tools"),
                        format!("unknown_qualified_tool:{tool_id}"),
                    ));
                }
            }
            check_receipt_budget::validate_check_budget(
                check,
                &self.qualified_tools,
                file,
                &format!("checks[{index}]"),
            )?;
            if !check_ids.insert(check.id.as_str()) {
                return Err(ContractError::config(file, "checks", "duplicate_check_id"));
            }
        }
        Ok(())
    }

    fn check_native_checks(&self, file: &str) -> Result<(), ContractError> {
        let mut ids: std::collections::BTreeSet<&str> =
            self.checks.iter().map(|check| check.id.as_str()).collect();
        for (index, check) in self.native_checks.iter().enumerate() {
            check.validate(file, &format!("native_checks[{index}]"))?;
            if !ids.insert(check.id.as_str()) {
                return Err(ContractError::config(
                    file,
                    "native_checks",
                    "duplicate_check_id",
                ));
            }
        }
        Ok(())
    }
}

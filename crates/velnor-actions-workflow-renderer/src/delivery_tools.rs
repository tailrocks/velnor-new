//! Neutral exact delivery helper setup; compiled SDK owns tool selection and execution.

use crate::{MiseSetup, RenderError, Yaml, mise_setup_step};
use std::collections::BTreeMap;
use velnor_actions_contract::{CompiledSourceHelper, ToolCacheDomain};

/// Compiled tool pins and preparation supplied by the SDK orchestration boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryToolContext {
    /// Actual-host qualified acquisition records.
    pub mise: MiseSetup,
    /// Exact catalog Python identity, for cross-context consistency checks.
    pub python_version: String,
    /// Exact catalog Gh identity, for cross-context consistency checks.
    pub gh_version: String,
    /// Exact source-owned catalog preparation record, never raw install argv.
    pub preparation: CompiledSourceHelper,
}

impl DeliveryToolContext {
    /// Validate neutral identity shape; SDK factory separately reconstructs authority.
    /// # Errors
    /// Rejects malformed identities or absent preparation bindings.
    pub fn validate(&self) -> Result<(), RenderError> {
        self.mise.validate()?;
        self.preparation
            .invocation()
            .validate()
            .map_err(RenderError::Contract)?;
        for (tool, version) in [("python", &self.python_version), ("gh", &self.gh_version)] {
            let parts = version.split('.').collect::<Vec<_>>();
            if parts.len() != 3
                || parts.iter().any(|part| {
                    part.is_empty()
                        || !part.bytes().all(|byte| byte.is_ascii_digit())
                        || part.len() > 1 && part.starts_with('0')
                })
            {
                return Err(RenderError::BadCommand(format!(
                    "delivery_tool_version:{tool}"
                )));
            }
        }
        Ok(())
    }

    /// Serialize exact acquisition and preparation for the finalized enclosing job.
    /// # Errors
    /// Rejects missing host/domain authority, markers or registry mismatches.
    pub fn setup_steps(
        &self,
        generator_version: &str,
        actual_runs_on: &str,
    ) -> Result<Vec<Yaml>, RenderError> {
        self.validate()?;
        let bootstrap = self.mise.bootstrap(ToolCacheDomain::Full, actual_runs_on)?;
        let acquisition = crate::source_helper::source_helper_step_to_yaml(
            &mise_setup_step(&self.mise, ToolCacheDomain::Full, actual_runs_on)?,
            std::slice::from_ref(&bootstrap.helper),
            generator_version,
            actual_runs_on,
        )?;
        let preparation = crate::source_helper::source_helper_step(
            "Prepare exact delivery tools",
            &self.preparation,
            self.preparation.environment().clone(),
        )?;
        let preparation = crate::source_helper::source_helper_step_to_yaml(
            &preparation,
            std::slice::from_ref(&self.preparation),
            generator_version,
            actual_runs_on,
        )?;
        Ok(vec![acquisition, preparation])
    }

    /// Canonical default isolation values for trusted domain data projections.
    #[must_use]
    pub fn isolation_env_pairs(&self) -> BTreeMap<String, String> {
        velnor_actions_contract::workflow::observer::isolation_env()
    }

    /// YAML form of the neutral default isolation values.
    #[must_use]
    pub fn isolation_env(&self) -> Yaml {
        Yaml::Map(
            self.isolation_env_pairs()
                .into_iter()
                .map(|(key, value)| (key, Yaml::str(value)))
                .collect(),
        )
    }
}

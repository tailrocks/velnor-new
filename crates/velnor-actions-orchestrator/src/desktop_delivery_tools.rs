//! Compose SDK-qualified native tools into neutral preparation and execution records.

use std::collections::BTreeMap;
use velnor_actions_contract::{CompiledSourceHelper, Step, ToolCacheDomain};
use velnor_actions_mise::catalog::{
    delivery_tools, mise_acquisition,
    native_desktop::{
        self, NativeDesktopKind, NativeDesktopProfile, NativeDesktopProfileTools,
        NativeTrustedOperation,
    },
    qualification::DistributionHost,
};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::{RenderError, delivery_tools::DeliveryToolContext};

/// Closed native compiler route, selected at generation time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeCompileDriver {
    /// Protected source-only Cargo compilation.
    Cargo,
    /// Qualified MBX verification compilation.
    Mbx,
}

/// Private SDK records; repository configuration supplies neither prefixes nor sources.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeDesktopToolContext {
    common: DeliveryToolContext,
    kind: NativeDesktopKind,
    generator_version: String,
    actual_runs_on: String,
    bootstrap: CompiledSourceHelper,
    profile: NativeDesktopProfileTools,
    driver: NativeCompileDriver,
}

impl NativeDesktopToolContext {
    /// Compose independently qualified macOS bootstrap and native compiler profiles.
    /// # Errors
    /// Rejects changed source records, unrelated host labels or unavailable owned bytes.
    pub fn compiled(
        common: DeliveryToolContext,
        kind: NativeDesktopKind,
        generator_version: &str,
        actual_runs_on: &str,
    ) -> Result<Self, RenderError> {
        Self::compiled_for_driver(
            common,
            kind,
            generator_version,
            actual_runs_on,
            NativeCompileDriver::Cargo,
        )
    }

    /// Compose only the explicitly requested verification profile.
    /// # Errors
    /// Rejects missing source-qualified Mise/MBX authority or changed host bindings.
    pub fn compiled_verification(
        common: DeliveryToolContext,
        kind: NativeDesktopKind,
        generator_version: &str,
        actual_runs_on: &str,
    ) -> Result<Self, RenderError> {
        Self::compiled_for_driver(
            common,
            kind,
            generator_version,
            actual_runs_on,
            NativeCompileDriver::Mbx,
        )
    }

    fn compiled_for_driver(
        common: DeliveryToolContext,
        kind: NativeDesktopKind,
        generator_version: &str,
        actual_runs_on: &str,
        driver: NativeCompileDriver,
    ) -> Result<Self, RenderError> {
        if !matches!(actual_runs_on, "macos-26" | "macos-15") {
            return Err(invalid("native_actual_host_not_macos_arm64"));
        }
        let bootstrap = mise_acquisition::helper_for_domain(
            ToolCacheDomain::Full,
            DistributionHost::MacosArm64,
            generator_version,
        )
        .map_err(sdk)?;
        let value = Self {
            common,
            kind,
            generator_version: generator_version.to_owned(),
            actual_runs_on: actual_runs_on.to_owned(),
            bootstrap,
            profile: native_desktop::profile_tools(kind, sdk_profile(driver), generator_version)
                .map_err(sdk)?,
            driver,
        };
        value.validate()?;
        Ok(value)
    }

    /// Reconstruct complete SDK authority, not just registry membership or selectors.
    /// # Errors
    /// Rejects any difference in source, invocation, profile, environment or host binding.
    pub fn validate(&self) -> Result<(), RenderError> {
        self.common.validate()?;
        let catalog = ToolCatalog::pinned();
        if self.common.python_version != catalog.version(PinnedTool::Python)
            || self.common.gh_version != catalog.version(PinnedTool::Gh)
            || delivery_tools::preparation(DistributionHost::MacosArm64, &self.generator_version)
                .map_err(sdk)?
                != self.common.preparation
            || self
                .common
                .mise
                .bootstrap(ToolCacheDomain::Full, &self.actual_runs_on)?
                .helper
                != self.bootstrap
            || mise_acquisition::helper_for_domain(
                ToolCacheDomain::Full,
                DistributionHost::MacosArm64,
                &self.generator_version,
            )
            .map_err(sdk)?
                != self.bootstrap
        {
            return Err(invalid("native_bootstrap_authority_changed"));
        }
        native_desktop::validate_profile_tools(
            self.kind,
            sdk_profile(self.driver),
            &self.profile,
            &self.generator_version,
        )
        .map_err(sdk)
    }

    /// Actual finalized native job runner label.
    #[must_use]
    pub fn actual_runs_on(&self) -> &str {
        &self.actual_runs_on
    }

    /// Exact scoped Rust compiler; no renderer-selected version.
    #[must_use]
    pub fn rust_version(&self) -> &str {
        self.profile.rust_version()
    }

    /// Neutral common tools with independently qualified host bootstrap.
    #[must_use]
    pub const fn common(&self) -> &DeliveryToolContext {
        &self.common
    }

    /// Preparation steps and their exact owner registry; renderer handles YAML.
    /// # Errors
    /// Rejects invalid or unavailable qualification.
    pub fn preparation_steps(
        &self,
        driver: NativeCompileDriver,
    ) -> Result<(Vec<Step>, Vec<CompiledSourceHelper>), RenderError> {
        self.validate()?;
        let preparation = self.profile(driver)?.preparation();
        let records = vec![self.bootstrap.clone(), preparation.clone()];
        let steps = [
            ("Acquire qualified Mise", &self.bootstrap),
            ("Prepare exact native desktop tools", preparation),
        ]
        .into_iter()
        .map(|(name, record)| {
            velnor_actions_workflow_renderer::source_helper::source_helper_step(
                name,
                record,
                record.environment().clone(),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
        Ok((steps, records))
    }

    /// Bind a complete compiled operation source to the checked SDK execution recipe.
    /// # Errors
    /// Rejects changed SDK authority or conflicting operation environment.
    pub fn bind_execution(
        &self,
        record: CompiledSourceHelper,
        driver: NativeCompileDriver,
    ) -> Result<CompiledSourceHelper, RenderError> {
        self.validate()?;
        record
            .with_execution_recipe(self.profile(driver)?.execution().clone())
            .map_err(RenderError::Contract)
    }

    /// Bind a fixed trusted operation to its separate credential-scoped SDK recipe.
    /// # Errors
    /// Rejects verification profiles or changed SDK privilege/owner authority.
    pub fn bind_trusted_execution(
        &self,
        record: CompiledSourceHelper,
        operation: NativeTrustedOperation,
    ) -> Result<CompiledSourceHelper, RenderError> {
        self.validate()?;
        if self.driver != NativeCompileDriver::Cargo {
            return Err(invalid("native_credentials_forbidden_in_verification"));
        }
        let recipe =
            native_desktop::operation_recipe(self.kind, operation, &self.generator_version)
                .map_err(sdk)?;
        native_desktop::validate_operation_recipe(
            self.kind,
            operation,
            &recipe,
            &self.generator_version,
        )
        .map_err(sdk)?;
        record
            .with_execution_recipe(recipe)
            .map_err(RenderError::Contract)
    }

    /// Anonymous compiler environment for typed domain input composition.
    #[must_use]
    pub fn isolation_env_pairs(&self) -> BTreeMap<String, String> {
        self.profile.execution().environment().clone()
    }

    fn profile(
        &self,
        driver: NativeCompileDriver,
    ) -> Result<&NativeDesktopProfileTools, RenderError> {
        if driver != self.driver {
            return Err(invalid("native_context_profile_not_requested"));
        }
        Ok(&self.profile)
    }
}

fn sdk(error: velnor_actions_mise::MiseError) -> RenderError {
    invalid(&error.to_string())
}
fn invalid(problem: &str) -> RenderError {
    RenderError::InvalidWorkflow(problem.to_owned())
}

fn sdk_profile(driver: NativeCompileDriver) -> NativeDesktopProfile {
    match driver {
        NativeCompileDriver::Cargo => NativeDesktopProfile::Source,
        NativeCompileDriver::Mbx => NativeDesktopProfile::Verification,
    }
}

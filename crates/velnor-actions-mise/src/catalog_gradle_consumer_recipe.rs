//! Anonymous SDK execution envelope for the sealed consumer installation.

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::native_tools::CompiledNativeExecRecipe;

use super::super::qualification::{
    DistributionRequirement, DistributionTool, QualifiedDistribution, qualified_toolset_digest,
};
use super::{GradleConsumerContext, GradleConsumerRole};
use crate::MiseError;

impl GradleConsumerContext {
    /// Fixed anonymous execution envelope for Native's compiled source owner.
    /// This generation-time binding requires runtime receipt admission or cold
    /// preparation before any mutable installed launch closure executes.
    /// # Errors
    /// Rejects malformed execution or environment bindings.
    pub fn execution_recipe(&self) -> Result<CompiledNativeExecRecipe, MiseError> {
        let mut environment: BTreeMap<_, _> = [
            ("HOME", "${{ runner.temp }}/velnor/gradle-source/home"),
            ("PATH", "/usr/bin:/bin:/usr/sbin:/sbin"),
            ("RUNNER_TEMP", "${{ runner.temp }}"),
            ("TMPDIR", "${{ runner.temp }}"),
            ("LC_ALL", "C"),
        ]
        .into_iter()
        .chain(crate::ISOLATION_ENV)
        .chain(crate::NO_AUTO_INSTALL_ENV)
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
        environment.extend(self.environment());
        let mise = QualifiedDistribution::require_for_generator(
            DistributionTool::Mise,
            self.host(),
            DistributionRequirement::RequiresNoMiserc,
        )?;
        environment.insert(
            "VELNOR_MISE_SHA256".to_owned(),
            mise.binary_sha256().to_owned(),
        );
        let mut records = self.records();
        records.push(mise);
        environment.insert(
            "VELNOR_QUALIFIED_TOOL_IDENTITY".to_owned(),
            format!("qualified-tools@{}", qualified_toolset_digest(&records)),
        );
        for (name, role) in [
            ("VELNOR_GRADLE_ENGINE", GradleConsumerRole::Engine),
            ("VELNOR_GRADLE_BOOTSTRAP", GradleConsumerRole::Bootstrap),
            ("VELNOR_GRADLE_JAVA", GradleConsumerRole::Java),
        ] {
            environment.insert(name.to_owned(), self.launch(role).executable());
        }
        let selectors = self.selectors();
        let mut prefix = vec!["/usr/bin/env".to_owned(), "-i".to_owned()];
        prefix.extend(environment.iter().map(|(key, value)| {
            format!(
                "{key}={}",
                value.replace("${{ runner.temp }}", "$RUNNER_TEMP")
            )
        }));
        prefix.push(
            format!("{}/bin/mise", self.owned_root()).replace("${{ runner.temp }}", "$RUNNER_TEMP"),
        );
        prefix.extend(crate::MISE_GLOBAL_FLAGS.into_iter().map(str::to_owned));
        prefix.push("exec".to_owned());
        prefix.extend(selectors.iter().cloned());
        prefix.push("--".to_owned());
        CompiledNativeExecRecipe::compiled(prefix, environment, selectors)
            .map_err(|error| super::super::tool_prepare::contract(&error))
    }

    /// Require the exact anonymous execution envelope from this sealed profile.
    /// # Errors
    /// Rejects changed paths, tools, identities, credentials or environment.
    pub fn validate_recipe(&self, recipe: &CompiledNativeExecRecipe) -> Result<(), MiseError> {
        if &self.execution_recipe()? != recipe {
            return Err(super::invalid());
        }
        Ok(())
    }
}

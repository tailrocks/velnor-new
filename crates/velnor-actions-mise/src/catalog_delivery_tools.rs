//! Closed Python/Gh delivery envelopes, qualified for the actual execution host.

use super::qualification::DistributionHost;
use crate::{MISE_GLOBAL_FLAGS, MiseError, PinnedTool, TOOL_COMMAND_SEPARATOR, ToolCatalog};
use std::collections::BTreeMap;
use velnor_actions_contract::workflow::native_tools::{
    CompiledNativeExecRecipe, NativeCredentialScope,
};

#[path = "catalog_release_prepare_tools.rs"]
mod release_prepare;
pub use release_prepare::{
    RELEASE_PREPARE_CONFIG, RELEASE_SEMVER_CHECKS_VERSION, rust_release_prepare_tools,
};

#[path = "catalog_source_snapshot_tools.rs"]
mod source_snapshot;
pub use source_snapshot::{
    SOURCE_SNAPSHOT_GH_ENV, SourceSnapshotTools, source_snapshot_tools,
    validate_source_snapshot_tools,
};

#[path = "catalog_admission_context.rs"]
mod admission;
pub use admission::{
    ADMISSION_PLANNING_GH_ENV, AdmissionTools, admission_tools, validate_admission_tools,
};

#[path = "catalog_source_intent_control.rs"]
mod source_intent_control;
pub use source_intent_control::{
    SourceIntentControlTools, source_intent_control_tools, validate_source_intent_control_tools,
};

/// Exact common helper execution without Rust, MBX or platform-specific build tools.
/// # Errors
/// Rejects unsupported credential purposes and unpublished owned host distributions.
pub fn execution_recipe(
    host: DistributionHost,
    scope: NativeCredentialScope,
) -> Result<CompiledNativeExecRecipe, MiseError> {
    if scope == NativeCredentialScope::AppleSigning {
        return Err(invalid("delivery_apple_scope_requires_native_owner"));
    }
    let catalog = ToolCatalog::pinned();
    let mut environment = super::native_tool_context::qualified_native_execution_environment(
        &catalog,
        host,
        velnor_actions_contract::ToolCacheDomain::Full,
        &[PinnedTool::Python, PinnedTool::Gh],
    )?;
    for (key, value) in [
        ("MISE_CONFIG_DIR", "${{ runner.temp }}/velnor/mise-config"),
        ("MISE_CACHE_DIR", "${{ runner.temp }}/velnor/mise-cache"),
        ("MISE_STATE_DIR", "${{ runner.temp }}/velnor/mise-state"),
        ("HOME", "${{ runner.temp }}/velnor/delivery-home"),
        ("RUNNER_TEMP", "${{ runner.temp }}"),
        ("PYTHONNOUSERSITE", "1"),
    ] {
        environment.insert(key.to_owned(), value.to_owned());
    }
    bind_credentials(&mut environment, scope);
    recipe_from_authority(
        &catalog,
        host,
        &[PinnedTool::Python, PinnedTool::Gh],
        environment,
        scope,
    )
}

fn bind_credentials(environment: &mut BTreeMap<String, String>, scope: NativeCredentialScope) {
    if matches!(
        scope,
        NativeCredentialScope::GithubReadOnly
            | NativeCredentialScope::GithubIssueWrite
            | NativeCredentialScope::GithubReleasePublish
            | NativeCredentialScope::OciRegistryPublish
    ) {
        environment.insert("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned());
    }
    if scope == NativeCredentialScope::OciRegistryPublish {
        environment.insert(
            "DOCKER_CONFIG".to_owned(),
            "${{ runner.temp }}/velnor/oci-docker".to_owned(),
        );
    }
    if scope == NativeCredentialScope::AptSigning {
        for key in ["APT_GPG_PRIVATE_KEY", "APT_GPG_PASSPHRASE"] {
            environment.insert(key.to_owned(), format!("${{{{ secrets.{key} }}}}"));
        }
    }
    if scope == NativeCredentialScope::RustRegistryPublishBootstrap {
        environment.insert(
            "CARGO_REGISTRY_TOKEN".to_owned(),
            "${{ secrets.CARGO_REGISTRY_TOKEN }}".to_owned(),
        );
    }
    if scope == NativeCredentialScope::RustRegistryPublishOidc {
        for key in [
            "ACTIONS_ID_TOKEN_REQUEST_URL",
            "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
        ] {
            environment.insert(key.to_owned(), format!("${{{{ env.{key} }}}}"));
        }
    }
}

/// Exact host-qualified common Python/Gh preparation record.
/// # Errors
/// Rejects unpublished owned host distributions or altered tool authority.
pub fn preparation(
    host: DistributionHost,
    generator_version: &str,
) -> Result<velnor_actions_contract::CompiledSourceHelper, MiseError> {
    let catalog = ToolCatalog::pinned();
    let _authority = super::native_tool_context::qualified_native_execution_environment(
        &catalog,
        host,
        velnor_actions_contract::ToolCacheDomain::Full,
        &[PinnedTool::Python, PinnedTool::Gh],
    )?;
    let mut selectors = catalog.native_tool_specs(host, &[PinnedTool::Python, PinnedTool::Gh])?;
    selectors.sort();
    super::tool_prepare::helper_for_tools(
        &catalog,
        velnor_actions_contract::ToolCacheDomain::Full,
        host,
        &selectors,
        generator_version,
    )
}

/// Semantic SDK admission uses exact compiled reconstruction, never selector parsing.
/// # Errors
/// Rejects any changed source qualification, host, token binding, prefix or environment.
pub fn validate_recipe(
    host: DistributionHost,
    scope: NativeCredentialScope,
    recipe: &CompiledNativeExecRecipe,
) -> Result<(), MiseError> {
    if &execution_recipe(host, scope)? != recipe {
        return Err(invalid("delivery_execution_authority_changed"));
    }
    Ok(())
}

fn invalid(problem: &str) -> MiseError {
    MiseError::Contract {
        problem: problem.to_owned(),
    }
}

/// Root Linux release inspection tools, distinct from the native desktop compiler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustReleaseTools {
    preparation: velnor_actions_contract::CompiledSourceHelper,
    execution: CompiledNativeExecRecipe,
}

impl RustReleaseTools {
    /// Exact source-bound Rust/Python/Gh installation record.
    #[must_use]
    pub const fn preparation(&self) -> &velnor_actions_contract::CompiledSourceHelper {
        &self.preparation
    }

    /// Root compiler envelope for the immutable release inspection closure.
    #[must_use]
    pub const fn execution(&self) -> &CompiledNativeExecRecipe {
        &self.execution
    }
}

/// Compile the closed root release inspection footprint on its supported host.
/// # Errors
/// Rejects non-root hosts, unsupported privileges, and absent owned distributions.
pub fn rust_release_tools(
    host: DistributionHost,
    scope: NativeCredentialScope,
    generator_version: &str,
) -> Result<RustReleaseTools, MiseError> {
    if host != DistributionHost::LinuxAmd64 {
        return Err(invalid("rust_release_host_unsupported"));
    }
    if !matches!(
        scope,
        NativeCredentialScope::Anonymous | NativeCredentialScope::GithubReadOnly
    ) {
        return Err(invalid("rust_release_scope_unsupported"));
    }
    let catalog = ToolCatalog::pinned();
    let tools = vec![catalog.compiler_tool(), PinnedTool::Python, PinnedTool::Gh];
    let install = crate::PreparePinnedTools::new(tools.clone(), crate::ToolHomes::runner_temp())?;
    let install: Vec<String> = install
        .argv_for_host(&catalog, host)?
        .into_iter()
        .map(|arg| {
            arg.into_string()
                .map_err(|_| invalid("rust_release_non_utf8_authority"))
        })
        .collect::<Result<_, _>>()?;
    let preparation = super::rust_prepare::helper_for_install(
        &catalog,
        super::rust_prepare::RustPrepareDomain::Tools,
        &install,
        generator_version,
    )?;
    let base = execution_recipe(host, scope)?;
    let mut environment = base.environment().clone();
    environment.extend(preparation.environment().clone());
    for (key, value) in crate::ToolHomes::runner_temp().exec_env(&catalog) {
        environment.insert(
            key.into_string()
                .map_err(|_| invalid("rust_release_non_utf8_authority"))?,
            value
                .into_string()
                .map_err(|_| invalid("rust_release_non_utf8_authority"))?,
        );
    }
    environment.extend(super::rust_prepare::qualified_exec_environment_for_tools(
        &catalog, host, &tools,
    )?);
    let execution = recipe_from_authority(&catalog, host, &tools, environment, scope)?;
    Ok(RustReleaseTools {
        preparation,
        execution,
    })
}

/// Reconstruct all root preparation source and execution authority.
/// # Errors
/// Rejects any changed recipe, installation, scope, host, or source bytes.
pub fn validate_rust_release_tools(
    host: DistributionHost,
    scope: NativeCredentialScope,
    generator_version: &str,
    record: &RustReleaseTools,
) -> Result<(), MiseError> {
    if &rust_release_tools(host, scope, generator_version)? != record {
        return Err(invalid("rust_release_authority_changed"));
    }
    Ok(())
}

fn recipe_from_authority(
    catalog: &ToolCatalog,
    host: DistributionHost,
    tools: &[PinnedTool],
    environment: BTreeMap<String, String>,
    scope: NativeCredentialScope,
) -> Result<CompiledNativeExecRecipe, MiseError> {
    let mut prefix = vec!["/usr/bin/env".to_owned(), "-i".to_owned()];
    prefix.extend(environment.iter().map(|(key, value)| {
        let value = if scope.allowed_keys().contains(&key.as_str()) {
            format!("${key}")
        } else {
            value.replace("${{ runner.temp }}", "$RUNNER_TEMP")
        };
        format!("{key}={value}")
    }));
    prefix.extend(managed_exec_prefix(catalog, host, tools)?);
    CompiledNativeExecRecipe::compiled_for_scope(
        prefix,
        environment,
        catalog.native_tool_specs(host, tools)?,
        scope,
    )
    .map_err(|error| invalid(&error.to_string()))
}

/// Pure canonical managed-tool prefix; caller inventories contain only typed catalog slots.
pub(super) fn managed_exec_prefix(
    catalog: &ToolCatalog,
    host: DistributionHost,
    tools: &[PinnedTool],
) -> Result<Vec<String>, MiseError> {
    Ok(
        std::iter::once("$RUNNER_TEMP/velnor/mise/bin/mise".to_owned())
            .chain(MISE_GLOBAL_FLAGS.map(str::to_owned))
            .chain(std::iter::once("exec".to_owned()))
            .chain(catalog.native_tool_specs(host, tools)?)
            .chain(std::iter::once(TOOL_COMMAND_SEPARATOR.to_owned()))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publisher_credentials_are_disjoint_and_exact() {
        for scope in [
            NativeCredentialScope::RustRegistryPublishOidc,
            NativeCredentialScope::RustRegistryPublishBootstrap,
            NativeCredentialScope::GithubReleasePublish,
        ] {
            let mut environment = BTreeMap::new();
            bind_credentials(&mut environment, scope);
            let keys: Vec<&str> = environment.keys().map(String::as_str).collect();
            let mut expected = scope.allowed_keys().to_vec();
            expected.sort_unstable();
            assert_eq!(keys, expected);
            for (key, value) in &environment {
                let expected = match key.as_str() {
                    "GH_TOKEN" => "${{ github.token }}".to_owned(),
                    "CARGO_REGISTRY_TOKEN" => "${{ secrets.CARGO_REGISTRY_TOKEN }}".to_owned(),
                    _ => format!("${{{{ env.{key} }}}}"),
                };
                assert_eq!(value, &expected);
            }
        }
    }

    #[test]
    fn pure_managed_prefix_uses_exact_catalog_and_isolation_flags() -> Result<(), MiseError> {
        let catalog = ToolCatalog::pinned();
        let host = DistributionHost::LinuxAmd64;
        let prefix = managed_exec_prefix(&catalog, host, &[PinnedTool::Gh])?;
        assert_eq!(
            prefix.first().map(String::as_str),
            Some("$RUNNER_TEMP/velnor/mise/bin/mise")
        );
        assert_eq!(
            &prefix[1..5],
            &["--no-config", "--no-env", "--no-hooks", "exec"]
        );
        let selectors = catalog.native_tool_specs(host, &[PinnedTool::Gh])?;
        assert_eq!(&prefix[5..6], selectors.as_slice());
        assert_eq!(prefix.last().map(String::as_str), Some("--"));
        assert!(catalog.tool_spec(PinnedTool::Python).is_err());
        Ok(())
    }

    #[test]
    fn root_release_rejects_foreign_host_before_qualification() {
        let result = rust_release_tools(
            DistributionHost::MacosArm64,
            NativeCredentialScope::GithubReadOnly,
            "0.1.0",
        );
        assert!(matches!(result, Err(MiseError::Contract { problem })
            if problem == "rust_release_host_unsupported"));
    }

    #[test]
    fn root_release_rejects_publishing_privilege_before_qualification() {
        let result = rust_release_tools(
            DistributionHost::LinuxAmd64,
            NativeCredentialScope::OciRegistryPublish,
            "0.1.0",
        );
        assert!(matches!(result, Err(MiseError::Contract { problem })
            if problem == "rust_release_scope_unsupported"));
    }
}

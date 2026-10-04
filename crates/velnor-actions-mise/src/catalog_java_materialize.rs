//! Compiled authority for installed Java homes and Gradle's persistent properties.

use std::collections::BTreeMap;

use velnor_actions_contract::{
    CompiledSourceHelper, HelperInvocation, SourceBoundHelper, SourceBoundOperation,
    ToolCacheDomain, compiled_source_sha256,
};

use super::{
    PinnedTool, ToolCatalog,
    qualification::{
        DistributionHost, DistributionRequirement, DistributionTool, QualifiedDistribution,
    },
};
use crate::MiseError;

/// Bind one installed catalog Java tool to a closed owned installation domain.
/// # Errors
/// Rejects unsupported domains and unavailable owner-qualified Mise distributions.
pub fn helper_for_domain(
    domain: ToolCacheDomain,
    version: &str,
) -> Result<CompiledSourceHelper, MiseError> {
    validate_domain(domain)?;
    let mise = QualifiedDistribution::require_for_generator(
        DistributionTool::Mise,
        DistributionHost::LinuxAmd64,
        DistributionRequirement::RequiresNoMiserc,
    )?;
    let java = QualifiedDistribution::require_native(
        DistributionTool::Java,
        DistributionHost::LinuxAmd64,
        ToolCatalog::pinned().version(PinnedTool::Java),
    )?;
    compiled(domain, version, &mise, &java)
}

fn compiled(
    domain: ToolCacheDomain,
    version: &str,
    mise: &QualifiedDistribution,
    java: &QualifiedDistribution,
) -> Result<CompiledSourceHelper, MiseError> {
    validate_domain(domain)?;
    let source = source(version, mise, java)?;
    let operation = SourceBoundOperation::JavaHomeMaterialize;
    let sha256 = compiled_source_sha256(source.as_bytes());
    let descriptor = SourceBoundHelper::compiled(operation, operation.path(), &sha256)
        .map_err(|error| contract(&error))?;
    let invocation = HelperInvocation::compiled(descriptor, vec![domain.name().to_owned()], vec![])
        .map_err(|error| contract(&error))?;
    let env = BTreeMap::from([
        ("MISE_DATA_DIR".to_owned(), domain.root().to_owned()),
        ("GRADLE_USER_HOME".to_owned(), GRADLE_HOME.to_owned()),
        (
            "VELNOR_QUALIFIED_TOOL_IDENTITY".to_owned(),
            format!(
                "qualified-tools@{}",
                super::qualification::qualified_toolset_digest(&[mise.clone(), java.clone()])
            ),
        ),
    ]);
    Ok(CompiledSourceHelper::compiled(invocation, source)
        .map_err(|error| contract(&error))?
        .with_environment(env))
}

/// Recover the exact compiled source and environment from a wire invocation.
/// # Errors
/// Rejects altered source, arguments, footprints, environment or unavailable qualification.
pub fn record_for_invocation(
    invocation: &HelperInvocation,
    env: &BTreeMap<String, String>,
    version: &str,
) -> Result<CompiledSourceHelper, MiseError> {
    record_with(invocation, env, version, helper_for_domain)
}

type Factory = fn(ToolCacheDomain, &str) -> Result<CompiledSourceHelper, MiseError>;

fn record_with(
    invocation: &HelperInvocation,
    env: &BTreeMap<String, String>,
    version: &str,
    factory: Factory,
) -> Result<CompiledSourceHelper, MiseError> {
    if invocation.descriptor().operation() != SourceBoundOperation::JavaHomeMaterialize
        || invocation.args().len() != 1
    {
        return Err(invalid());
    }
    let domain = domains()
        .into_iter()
        .find(|domain| invocation.args()[0] == domain.name())
        .ok_or_else(invalid)?;
    let expected = factory(domain, version)?;
    if expected.invocation() != invocation || expected.environment() != env {
        return Err(invalid());
    }
    Ok(expected)
}

fn source(
    version: &str,
    mise: &QualifiedDistribution,
    java: &QualifiedDistribution,
) -> Result<String, MiseError> {
    let java_configuration = java_configuration(java)?;
    let domains: BTreeMap<_, _> = domains()
        .into_iter()
        .map(|domain| {
            (
                domain.name(),
                serde_json::json!({
                    "mise": domain.root().trim_start_matches("${{ runner.temp }}/velnor/"),
                    "gradle": GRADLE_HOME.trim_start_matches("${{ runner.temp }}/velnor/"),
                }),
            )
        })
        .collect();
    let isolation: BTreeMap<_, _> = crate::ISOLATION_ENV
        .into_iter()
        .chain(crate::NO_AUTO_INSTALL_ENV)
        .collect();
    let configuration = serde_json::json!({
        "domains": domains,
        "java": java_configuration,
        "flags": crate::MISE_GLOBAL_FLAGS,
        "binary_sha256": mise.binary_sha256(),
        "isolation": isolation,
    });
    let body = format!(
        "set -euo pipefail\nexport PATH=/usr/bin:/bin:/usr/sbin:/sbin\ntest \"$#\" -eq 1\n/usr/bin/python3 -I -S - \"$1\" <<'VELNOR_JAVA_HOME_MATERIALIZE'\n{}\nconfiguration = json.loads({:?})\nmaterialize(__import__('sys').argv[1], configuration)\nVELNOR_JAVA_HOME_MATERIALIZE\n",
        include_str!("catalog_java_materialize.py"),
        configuration.to_string(),
    );
    velnor_actions_contract::generated_source(version, &body).map_err(|error| contract(&error))
}

fn java_configuration(java: &QualifiedDistribution) -> Result<serde_json::Value, MiseError> {
    if java.tool() != DistributionTool::Java || java.host() != DistributionHost::LinuxAmd64 {
        return Err(invalid());
    }
    let plan = java.required_install_plan()?;
    let home = plan
        .environment()
        .iter()
        .find(|entry| entry.name() == "JAVA_HOME")
        .ok_or_else(invalid)?;
    let relative_home = if home.relative_path().is_empty() {
        plan.root_relative_path().to_owned()
    } else {
        format!("{}/{}", plan.root_relative_path(), home.relative_path())
    };
    if java.required_installed_binary_path()? != format!("{relative_home}/bin/java") {
        return Err(invalid());
    }
    let launch: Vec<_> = java
        .launch_entries()
        .iter()
        .map(|entry| {
            let path = entry.installed_relative_path().ok_or_else(invalid)?;
            Ok(serde_json::json!({"path": path, "sha256": entry.sha256()}))
        })
        .collect::<Result<_, MiseError>>()?;
    Ok(serde_json::json!({
        "selector": java.selector(),
        "selection_version": java.selection_version(),
        "reported_version": java.version(),
        "install_root": plan.root_relative_path(),
        "home": relative_home,
        "launch": launch,
        "qualification": java.qualification_digest(),
    }))
}

const GRADLE_HOME: &str = "${{ runner.temp }}/velnor/native/gradle";

const fn domains() -> [ToolCacheDomain; 2] {
    [ToolCacheDomain::Full, ToolCacheDomain::GradleBootstrap]
}

fn validate_domain(domain: ToolCacheDomain) -> Result<(), MiseError> {
    domains()
        .contains(&domain)
        .then_some(())
        .ok_or_else(invalid)
}

fn invalid() -> MiseError {
    MiseError::Contract {
        problem: "invalid_java_home_materialization".to_owned(),
    }
}

fn contract(error: &velnor_actions_contract::ContractError) -> MiseError {
    MiseError::Contract {
        problem: error.to_string(),
    }
}

#[cfg(test)]
#[path = "catalog_java_materialize_tests.rs"]
mod tests;

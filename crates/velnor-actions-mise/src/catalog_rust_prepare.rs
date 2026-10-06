//! Source-bound Rust preparation, with one owner for construction and auditing.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    CompiledSourceHelper, HelperInvocation, SourceBoundHelper, SourceBoundOperation,
};

use super::{
    PinnedTool, ToolCatalog,
    qualification::{
        DistributionHost, DistributionRequirement, DistributionTool, QualifiedDistribution,
    },
    rust_desktop::RustCompilerRole,
};
use crate::{MISE_GLOBAL_FLAGS, MiseError};

#[path = "catalog_rust_prepare_body.rs"]
mod body;
pub(in crate::catalog) use body::{
    CANDIDATE_PRELUDE, CANDIDATE_ROOTS, candidate_archive_source, candidate_install_source,
};

#[path = "catalog_rust_prepare_exec_env.rs"]
mod exec_env;
pub use exec_env::qualified_exec_environment_for_tools;

#[path = "catalog_rust_prepare_receipt.rs"]
mod receipt;
pub use receipt::{RustReceiptPreparation, receipt_preparation_for_install};

/// The only Mise bootstrap locations a Rust helper can execute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RustPrepareDomain {
    /// Normal jobs use the full tools bootstrap.
    Tools,
    /// Early Plan uses its independent bootstrap while preparing full tools.
    PlanningBootstrap,
}

impl RustPrepareDomain {
    /// Closed bootstrap owner, independent of the Full Rust payload domain.
    #[must_use]
    pub const fn bootstrap_domain(self) -> velnor_actions_contract::ToolCacheDomain {
        match self {
            Self::Tools => velnor_actions_contract::ToolCacheDomain::Full,
            Self::PlanningBootstrap => velnor_actions_contract::ToolCacheDomain::Planning,
        }
    }

    /// Literal helper argument; never interpreted as shell code.
    #[must_use]
    pub const fn argument(self) -> &'static str {
        match self {
            Self::Tools => "tools",
            Self::PlanningBootstrap => "planning-bootstrap",
        }
    }
}

/// Bind an exact catalog installation to fixed generated source and arguments.
/// # Errors
/// Rejects noncanonical catalogs, foreign selectors, and malformed install argv.
pub fn helper_for_install(
    catalog: &ToolCatalog,
    domain: RustPrepareDomain,
    install: &[String],
    generator_version: &str,
) -> Result<CompiledSourceHelper, MiseError> {
    let role = role_for_catalog(catalog)?;
    let host = distribution_host(role);
    let mise = QualifiedDistribution::require_for_generator(
        DistributionTool::Mise,
        host,
        DistributionRequirement::RequiresNoMiserc,
    )?;
    let mut qualifications = vec![mise.clone()];
    let mbx = if install.contains(&catalog.tool_spec(PinnedTool::MrBoxington)?) {
        let record = QualifiedDistribution::require_for_generator(
            DistributionTool::Mbx,
            host,
            DistributionRequirement::MbxTransport,
        )?;
        qualifications.push(record.clone());
        Some(record)
    } else {
        None
    };
    compiled_install(
        catalog,
        domain,
        install,
        generator_version,
        &mise,
        mbx.as_ref(),
        &qualifications,
    )
}

fn compiled_install(
    catalog: &ToolCatalog,
    domain: RustPrepareDomain,
    install: &[String],
    generator_version: &str,
    mise: &QualifiedDistribution,
    mbx: Option<&QualifiedDistribution>,
    qualifications: &[QualifiedDistribution],
) -> Result<CompiledSourceHelper, MiseError> {
    let role = role_for_catalog(catalog)?;
    let prefix = install_prefix();
    let selectors = install
        .strip_prefix(prefix.as_slice())
        .ok_or_else(invalid)?;
    let mut qualifications = qualifications.to_vec();
    qualifications.extend(validate_selectors(catalog, selectors)?);
    let source = body::source(role, generator_version, mise, mbx)?;
    let sha256 = velnor_actions_contract::compiled_source_sha256(source.as_bytes());
    let (operation, path) = binding(role);
    let descriptor =
        SourceBoundHelper::compiled(operation, path, &sha256).map_err(|error| contract(&error))?;
    let args = std::iter::once(domain.argument().to_owned())
        .chain(selectors.iter().cloned())
        .collect();
    let invocation = HelperInvocation::compiled(descriptor, args, selectors.to_vec())
        .map_err(|error| contract(&error))?;
    Ok(CompiledSourceHelper::compiled(invocation, source)
        .map_err(|error| contract(&error))?
        .with_environment(environment(catalog, selectors, &qualifications)?))
}

/// Recover install argv only from an exact owner-generated invocation.
#[must_use]
pub fn install_argv(invocation: &HelperInvocation, generator_version: &str) -> Option<Vec<String>> {
    install_argv_with(invocation, generator_version, helper_for_install)
}

type InstallFactory =
    fn(&ToolCatalog, RustPrepareDomain, &[String], &str) -> Result<CompiledSourceHelper, MiseError>;

fn install_argv_with(
    invocation: &HelperInvocation,
    generator_version: &str,
    factory: InstallFactory,
) -> Option<Vec<String>> {
    let role = match invocation.descriptor().operation() {
        SourceBoundOperation::RustPrepareRootLinux => RustCompilerRole::RootLinux,
        SourceBoundOperation::RustPrepareDesktopMac => RustCompilerRole::DesktopMac,
        SourceBoundOperation::RustPrepareDesktopSourceMac => RustCompilerRole::DesktopSourceMac,
        SourceBoundOperation::RustPrepareReleaseMac => RustCompilerRole::ReleaseMac,
        _ => return None,
    };
    let domain = match invocation.args().first()?.as_str() {
        "tools" => RustPrepareDomain::Tools,
        "planning-bootstrap" => RustPrepareDomain::PlanningBootstrap,
        _ => return None,
    };
    let catalog = catalog_for_role(role).ok()?;
    let mut install = install_prefix();
    install.extend(invocation.args()[1..].iter().cloned());
    let expected = factory(&catalog, domain, &install, generator_version).ok()?;
    (expected.invocation() == invocation).then_some(install)
}

/// Rebuild compiled authority from IR without trusting its source or environment.
/// # Errors
/// Rejects altered invocations, tool footprints, or owner environment values.
pub fn record_for_invocation(
    invocation: &HelperInvocation,
    env: &BTreeMap<String, String>,
    generator_version: &str,
) -> Result<CompiledSourceHelper, MiseError> {
    record_for_invocation_with(invocation, env, generator_version, helper_for_install)
}

fn record_for_invocation_with(
    invocation: &HelperInvocation,
    env: &BTreeMap<String, String>,
    generator_version: &str,
    factory: InstallFactory,
) -> Result<CompiledSourceHelper, MiseError> {
    let install = install_argv_with(invocation, generator_version, factory).ok_or_else(invalid)?;
    let role = match invocation.descriptor().operation() {
        SourceBoundOperation::RustPrepareRootLinux => RustCompilerRole::RootLinux,
        SourceBoundOperation::RustPrepareDesktopMac => RustCompilerRole::DesktopMac,
        SourceBoundOperation::RustPrepareDesktopSourceMac => RustCompilerRole::DesktopSourceMac,
        SourceBoundOperation::RustPrepareReleaseMac => RustCompilerRole::ReleaseMac,
        _ => return Err(invalid()),
    };
    let domain = match invocation.args().first().map(String::as_str) {
        Some("tools") => RustPrepareDomain::Tools,
        Some("planning-bootstrap") => RustPrepareDomain::PlanningBootstrap,
        _ => return Err(invalid()),
    };
    let record = factory(
        &catalog_for_role(role)?,
        domain,
        &install,
        generator_version,
    )?;
    if record.environment() != env {
        return Err(invalid());
    }
    Ok(record)
}

fn role_for_catalog(catalog: &ToolCatalog) -> Result<RustCompilerRole, MiseError> {
    for role in [
        RustCompilerRole::RootLinux,
        RustCompilerRole::DesktopMac,
        RustCompilerRole::DesktopSourceMac,
        RustCompilerRole::ReleaseMac,
    ] {
        if catalog == &catalog_for_role(role)? {
            return Ok(role);
        }
    }
    Err(invalid())
}

fn catalog_for_role(role: RustCompilerRole) -> Result<ToolCatalog, MiseError> {
    let catalog = ToolCatalog::pinned();
    match role {
        RustCompilerRole::RootLinux => Ok(catalog),
        RustCompilerRole::ReleaseMac => ToolCatalog::for_release_host(DistributionHost::MacosArm64),
        RustCompilerRole::DesktopMac => catalog.for_native_kind("native_xcode_project_ci"),
        RustCompilerRole::DesktopSourceMac => {
            catalog.for_native_source_kind("native_xcode_project_ci")
        }
    }
}

fn binding(role: RustCompilerRole) -> (SourceBoundOperation, &'static str) {
    match role {
        RustCompilerRole::RootLinux => (
            SourceBoundOperation::RustPrepareRootLinux,
            ".github/velnor/rust_prepare_root_linux.sh",
        ),
        RustCompilerRole::DesktopMac => (
            SourceBoundOperation::RustPrepareDesktopMac,
            ".github/velnor/rust_prepare_desktop_mac.sh",
        ),
        RustCompilerRole::DesktopSourceMac => (
            SourceBoundOperation::RustPrepareDesktopSourceMac,
            ".github/velnor/rust_prepare_desktop_source_mac.sh",
        ),
        RustCompilerRole::ReleaseMac => (
            SourceBoundOperation::RustPrepareReleaseMac,
            ".github/velnor/rust_prepare_release_mac.sh",
        ),
    }
}

fn install_prefix() -> Vec<String> {
    std::iter::once("mise")
        .chain(MISE_GLOBAL_FLAGS.iter().copied())
        .chain(std::iter::once("install"))
        .map(str::to_owned)
        .collect()
}

fn validate_selectors(
    catalog: &ToolCatalog,
    selectors: &[String],
) -> Result<Vec<QualifiedDistribution>, MiseError> {
    let allowed: BTreeSet<_> = PinnedTool::ALL
        .into_iter()
        .filter(|tool| {
            !ToolCatalog::requires_native_host(*tool)
                && (!matches!(tool, PinnedTool::Rust | PinnedTool::RustDesktop)
                    || *tool == catalog.compiler_tool())
        })
        .map(|tool| catalog.tool_spec(tool))
        .collect::<Result<_, _>>()?;
    let unique: BTreeSet<_> = selectors.iter().collect();
    if selectors.is_empty()
        || unique.len() != selectors.len()
        || !selectors.contains(&catalog.tool_spec(catalog.compiler_tool())?)
        || (catalog.rust_uses_mbx()
            && !selectors.contains(&catalog.tool_spec(PinnedTool::MrBoxington)?))
    {
        return Err(invalid());
    }
    let host = distribution_host(role_for_catalog(catalog)?);
    let mut qualifications = BTreeMap::new();
    for selector in selectors {
        if allowed.contains(selector) {
            continue;
        }
        let tool = PinnedTool::ALL
            .into_iter()
            .filter(|tool| ToolCatalog::requires_native_host(*tool))
            .find(|tool| {
                let slot = match tool {
                    PinnedTool::Java => "graalvm-community-jdk",
                    _ => tool.tool_name(),
                };
                selector.starts_with(&format!("http:{slot}["))
            })
            .ok_or_else(invalid)?;
        if catalog.native_tool_spec(host, tool)? != *selector {
            return Err(invalid());
        }
        let record = catalog.native_distribution(host, tool)?;
        qualifications.insert(record.qualification_digest(), record);
    }
    Ok(qualifications.into_values().collect())
}

fn environment(
    catalog: &ToolCatalog,
    selectors: &[String],
    qualifications: &[QualifiedDistribution],
) -> Result<BTreeMap<String, String>, MiseError> {
    let mut env = BTreeMap::new();
    for (key, value) in crate::ToolHomes::runner_temp().prepare_env(catalog) {
        env.insert(
            key.into_string().map_err(|_| invalid())?,
            value.into_string().map_err(|_| invalid())?,
        );
    }
    env.extend(
        crate::command::NO_AUTO_INSTALL_ENV.map(|(key, value)| (key.to_owned(), value.to_owned())),
    );
    let unique: BTreeSet<_> = selectors.iter().map(String::as_str).collect();
    let identity = velnor_actions_contract::digest_b3(
        unique.into_iter().collect::<Vec<_>>().join("\0").as_bytes(),
    );
    let bootstrap = super::rust_bootstrap::RustupBootstrap::for_host(catalog.rust_host());
    for (key, value) in [
        (
            "MISE_DATA_DIR",
            crate::runtime_paths::MISE_DATA_DIR.to_owned(),
        ),
        ("CARGO_HOME", "${{ runner.temp }}/velnor/cargo".to_owned()),
        ("RUSTUP_HOME", "${{ runner.temp }}/velnor/rustup".to_owned()),
        ("RUSTUP_VERSION", bootstrap.version().to_owned()),
        ("VELNOR_TOOL_CACHE_IDENTITY", format!("toolset@{identity}")),
        (
            "VELNOR_QUALIFIED_TOOL_IDENTITY",
            format!(
                "qualified-tools@{}",
                super::qualification::qualified_toolset_digest(qualifications)
            ),
        ),
        (
            "VELNOR_RUSTUP_IDENTITY",
            format!(
                "rustup@{}+sha256.{}+health.{}",
                bootstrap.version(),
                bootstrap.sha256(),
                super::rust_health::RUST_HEALTH_POLICY
            ),
        ),
    ] {
        env.insert(key.to_owned(), value);
    }
    Ok(env)
}

fn distribution_host(role: RustCompilerRole) -> DistributionHost {
    match role {
        RustCompilerRole::RootLinux => DistributionHost::LinuxAmd64,
        RustCompilerRole::DesktopMac
        | RustCompilerRole::DesktopSourceMac
        | RustCompilerRole::ReleaseMac => DistributionHost::MacosArm64,
    }
}

fn invalid() -> MiseError {
    MiseError::Contract {
        problem: "invalid_rust_prepare_invocation".to_owned(),
    }
}

fn contract(error: &velnor_actions_contract::ContractError) -> MiseError {
    MiseError::Contract {
        problem: error.to_string(),
    }
}

#[cfg(test)]
#[path = "catalog_rust_prepare_tests.rs"]
mod tests;

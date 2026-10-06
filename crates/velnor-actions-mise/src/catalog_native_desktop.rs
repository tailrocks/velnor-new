//! Closed native tool preparation and execution envelopes, built at generation time.

use std::ffi::OsString;

use velnor_actions_contract::{
    CompiledSourceHelper,
    workflow::native_tools::{CompiledNativeExecRecipe, NativeCredentialScope},
};

use super::{
    qualification::DistributionHost,
    rust_prepare::{self, RustPrepareDomain},
};
use crate::{MiseError, PinnedTool, PreparePinnedTools, ToolCatalog, ToolHomes};

/// Native workload whose catalog/compiler authority is closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeDesktopKind {
    /// Native Xcode application assembly.
    XcodeProject,
    /// Native Swift package validation.
    SwiftPackage,
}

impl NativeDesktopKind {
    const fn name(self) -> &'static str {
        match self {
            Self::XcodeProject => "native_xcode_project_ci",
            Self::SwiftPackage => "native_swift_package_ci",
        }
    }
}

/// Closed compiler/cache route, independent of repository version strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeDesktopProfile {
    /// Protected production compiles from source without MBX.
    Source,
    /// Validation uses the qualified MBX transport.
    Verification,
}

/// Trusted operations whose credentials never enter repository compilation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeTrustedOperation {
    /// Inspect immutable GitHub release metadata using the fixed read token.
    GithubReleaseInspection,
    /// Sign and notarize an already verified unsigned application.
    AppleSignAndNotarize,
}

/// Exact source-only execution envelope for one compiled trusted operation.
/// # Errors
/// Rejects unavailable owner qualification or malformed closed SDK authority.
pub fn operation_recipe(
    kind: NativeDesktopKind,
    operation: NativeTrustedOperation,
    generator_version: &str,
) -> Result<CompiledNativeExecRecipe, MiseError> {
    let tools = profile_tools(kind, NativeDesktopProfile::Source, generator_version)?;
    let base = tools.execution();
    let mut environment = base.environment().clone();
    let scope = match operation {
        NativeTrustedOperation::GithubReleaseInspection => {
            environment.insert("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned());
            NativeCredentialScope::GithubReadOnly
        }
        NativeTrustedOperation::AppleSignAndNotarize => NativeCredentialScope::AppleSigning,
    };
    CompiledNativeExecRecipe::compiled_for_scope(
        base.prefix().to_vec(),
        environment,
        base.installed_selectors().to_vec(),
        scope,
    )
    .map_err(|error| invalid(&error.to_string()))
}

/// Reconstruct trusted operation scope and exact SDK envelope before source binding.
/// # Errors
/// Rejects changed operation privilege, catalog pins, source qualification or environment.
pub fn validate_operation_recipe(
    kind: NativeDesktopKind,
    operation: NativeTrustedOperation,
    record: &CompiledNativeExecRecipe,
    generator_version: &str,
) -> Result<(), MiseError> {
    if &operation_recipe(kind, operation, generator_version)? != record {
        return Err(invalid("native_trusted_operation_authority_changed"));
    }
    Ok(())
}

/// SDK-owned preparation source and checked execution envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeDesktopProfileTools {
    preparation: CompiledSourceHelper,
    execution: CompiledNativeExecRecipe,
    rust_version: String,
}

impl NativeDesktopProfileTools {
    /// Exact source-bound tool preparation registry record.
    #[must_use]
    pub const fn preparation(&self) -> &CompiledSourceHelper {
        &self.preparation
    }
    /// Checked execution envelope with the same installation footprint.
    #[must_use]
    pub const fn execution(&self) -> &CompiledNativeExecRecipe {
        &self.execution
    }
    /// Exact scoped compiler version.
    #[must_use]
    pub fn rust_version(&self) -> &str {
        &self.rust_version
    }
}

/// Build the complete profile from compiled catalog and qualified distributions.
/// # Errors
/// Fails closed for missing owned distributions or malformed authority records.
pub fn profile_tools(
    kind: NativeDesktopKind,
    profile: NativeDesktopProfile,
    generator_version: &str,
) -> Result<NativeDesktopProfileTools, MiseError> {
    let catalog = catalog_for(kind, profile)?;
    let tools = tool_inventory(&catalog);
    let install = PreparePinnedTools::new(tools.clone(), ToolHomes::runner_temp())?;
    let install = utf8_argv(install.argv_for_host(&catalog, DistributionHost::MacosArm64)?)?;
    let preparation = rust_prepare::helper_for_install(
        &catalog,
        RustPrepareDomain::Tools,
        &install,
        generator_version,
    )?;
    let execution = recipe(&catalog, &tools, &preparation)?;
    Ok(NativeDesktopProfileTools {
        preparation,
        execution,
        rust_version: catalog.rustup_toolchain(),
    })
}

/// Exact SDK reconstruction, including preparation source/arguments/environment.
/// # Errors
/// Rejects forged selector, environment, prefix, or source authority.
pub fn validate_profile_tools(
    kind: NativeDesktopKind,
    profile: NativeDesktopProfile,
    record: &NativeDesktopProfileTools,
    generator_version: &str,
) -> Result<(), MiseError> {
    if &profile_tools(kind, profile, generator_version)? != record {
        return Err(invalid("native_profile_authority_mismatch"));
    }
    Ok(())
}

/// Admit a neutral recipe only after exact compiled SDK reconstruction.
/// # Errors
/// Rejects any difference in pinned tools, sanitized prefix, or owned environment.
pub fn validate_recipe(
    kind: NativeDesktopKind,
    profile: NativeDesktopProfile,
    recipe: &CompiledNativeExecRecipe,
    generator_version: &str,
) -> Result<(), MiseError> {
    if profile_tools(kind, profile, generator_version)?.execution() != recipe {
        return Err(invalid("native_exec_authority_mismatch"));
    }
    Ok(())
}

fn catalog_for(
    kind: NativeDesktopKind,
    profile: NativeDesktopProfile,
) -> Result<ToolCatalog, MiseError> {
    let catalog = ToolCatalog::pinned();
    match profile {
        NativeDesktopProfile::Source => catalog.for_native_source_kind(kind.name()),
        NativeDesktopProfile::Verification => catalog.for_native_kind(kind.name()),
    }
}

fn tool_inventory(catalog: &ToolCatalog) -> Vec<PinnedTool> {
    let mut tools = vec![catalog.compiler_tool()];
    if catalog.rust_uses_mbx() {
        tools.push(PinnedTool::MrBoxington);
    }
    tools.extend([
        PinnedTool::Python,
        PinnedTool::Gh,
        PinnedTool::Boltffi,
        PinnedTool::Xcodegen,
        PinnedTool::Nextest,
        PinnedTool::SwiftLint,
        PinnedTool::Periphery,
    ]);
    tools
}

fn recipe(
    catalog: &ToolCatalog,
    tools: &[PinnedTool],
    preparation: &CompiledSourceHelper,
) -> Result<CompiledNativeExecRecipe, MiseError> {
    let mut environment = preparation.environment().clone();
    for (key, value) in ToolHomes::runner_temp().exec_env(catalog) {
        environment.insert(utf8(key)?, utf8(value)?);
    }
    environment.extend([
        (
            "HOME".to_owned(),
            "${{ runner.temp }}/velnor/native-home".to_owned(),
        ),
        (
            "PATH".to_owned(),
            "/usr/bin:/bin:/usr/sbin:/sbin".to_owned(),
        ),
        ("RUNNER_TEMP".to_owned(), "${{ runner.temp }}".to_owned()),
        ("PYTHONNOUSERSITE".to_owned(), "1".to_owned()),
    ]);
    environment.extend(rust_prepare::qualified_exec_environment_for_tools(
        catalog,
        DistributionHost::MacosArm64,
        tools,
    )?);
    let mut prefix = vec!["/usr/bin/env".to_owned(), "-i".to_owned()];
    prefix.extend(environment.iter().map(|(key, value)| {
        format!(
            "{key}={}",
            value.replace("${{ runner.temp }}", "$RUNNER_TEMP")
        )
    }));
    prefix.extend(super::delivery_tools::managed_exec_prefix(
        catalog,
        DistributionHost::MacosArm64,
        tools,
    )?);
    CompiledNativeExecRecipe::compiled(
        prefix,
        environment,
        catalog.native_tool_specs(DistributionHost::MacosArm64, tools)?,
    )
    .map_err(|error| invalid(&error.to_string()))
}

fn utf8_argv(argv: Vec<OsString>) -> Result<Vec<String>, MiseError> {
    argv.into_iter().map(utf8).collect()
}

fn utf8(value: OsString) -> Result<String, MiseError> {
    value
        .into_string()
        .map_err(|_| invalid("native_non_utf8_authority"))
}

fn invalid(problem: &str) -> MiseError {
    MiseError::Contract {
        problem: problem.to_owned(),
    }
}

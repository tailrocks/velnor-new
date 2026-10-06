//! Exact invocation and emitted-source regression proofs.

use super::*;
use sha2::{Digest, Sha256};

// Unit-only official source fixture exercises envelope semantics. It never grants
// product qualification; the production gate below must reject these releases.
fn helper_for_install(
    catalog: &ToolCatalog,
    domain: RustPrepareDomain,
    install: &[String],
    version: &str,
) -> Result<CompiledSourceHelper, MiseError> {
    let mise = QualifiedDistribution::qualify_official(
        DistributionTool::Mise,
        distribution_host(role_for_catalog(catalog)?),
    )?;
    compiled_install(
        catalog,
        domain,
        install,
        version,
        &mise,
        None,
        std::slice::from_ref(&mise),
    )
}

fn install_argv(invocation: &HelperInvocation, version: &str) -> Option<Vec<String>> {
    install_argv_with(invocation, version, helper_for_install)
}

fn record_for_invocation(
    invocation: &HelperInvocation,
    env: &BTreeMap<String, String>,
    version: &str,
) -> Result<CompiledSourceHelper, MiseError> {
    record_for_invocation_with(invocation, env, version, helper_for_install)
}

fn install(catalog: &ToolCatalog) -> Result<Vec<String>, MiseError> {
    let mut argv = install_prefix();
    argv.extend(catalog.tool_specs(&[
        catalog.compiler_tool(),
        PinnedTool::MrBoxington,
        PinnedTool::Nextest,
    ])?);
    Ok(argv)
}

#[test]
fn both_roles_and_bootstrap_domains_round_trip_exact_install() -> Result<(), MiseError> {
    for role in [
        RustCompilerRole::RootLinux,
        RustCompilerRole::DesktopMac,
        RustCompilerRole::DesktopSourceMac,
        RustCompilerRole::ReleaseMac,
    ] {
        let catalog = catalog_for_role(role)?;
        let argv = install(&catalog)?;
        for domain in [
            RustPrepareDomain::Tools,
            RustPrepareDomain::PlanningBootstrap,
        ] {
            let record = helper_for_install(&catalog, domain, &argv, "0.1.0")?;
            for (key, value) in crate::command::NO_AUTO_INSTALL_ENV
                .into_iter()
                .chain([("RUSTUP_AUTO_INSTALL", "0")])
            {
                assert_eq!(
                    record.environment().get(key).map(String::as_str),
                    Some(value)
                );
                assert!(record.source().contains(&format!("export {key}={value}")));
            }
            assert_eq!(
                install_argv(record.invocation(), "0.1.0"),
                Some(argv.clone())
            );
            assert_eq!(record.invocation().args()[0], domain.argument());
            assert_eq!(
                record.invocation().installed_selectors(),
                &argv[install_prefix().len()..]
            );
            assert_eq!(
                record.invocation().descriptor().source_sha256(),
                Sha256::digest(record.source().as_bytes())
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            );
            assert!(record.source().contains("install \"$@\""));
            assert!(
                !record
                    .invocation()
                    .args()
                    .iter()
                    .any(|arg| arg.contains("$("))
            );
            assert!(install_argv(record.invocation(), "0.1.1").is_none());
            assert_eq!(
                record_for_invocation(record.invocation(), record.environment(), "0.1.0")?,
                record
            );
        }
    }
    Ok(())
}

#[test]
fn generator_rejects_absent_owned_qualification_without_upstream_fallback() -> Result<(), MiseError>
{
    for role in [
        RustCompilerRole::RootLinux,
        RustCompilerRole::DesktopMac,
        RustCompilerRole::DesktopSourceMac,
        RustCompilerRole::ReleaseMac,
    ] {
        let catalog = catalog_for_role(role)?;
        assert!(
            super::helper_for_install(
                &catalog,
                RustPrepareDomain::Tools,
                &install(&catalog)?,
                "0.1.0"
            )
            .is_err()
        );
    }
    Ok(())
}

#[test]
fn source_desktop_role_requires_no_mbx_or_cargo_wrapper() -> Result<(), MiseError> {
    let catalog = catalog_for_role(RustCompilerRole::DesktopSourceMac)?;
    let mut argv = install_prefix();
    argv.push(catalog.tool_spec(catalog.compiler_tool())?);
    let record = helper_for_install(&catalog, RustPrepareDomain::Tools, &argv, "0.1.0")?;
    assert_eq!(
        record.invocation().descriptor().operation(),
        SourceBoundOperation::RustPrepareDesktopSourceMac
    );
    assert!(!record.source().contains("VELNOR_WRAPPER_VERIFY"));
    assert!(!record.source().contains("mr_boxington=true"));
    assert_eq!(install_argv(record.invocation(), "0.1.0"), Some(argv));
    Ok(())
}

#[test]
fn release_mac_binds_root_compiler_without_mbx_dispatch() -> Result<(), MiseError> {
    let catalog = catalog_for_role(RustCompilerRole::ReleaseMac)?;
    let mut argv = install_prefix();
    argv.push(catalog.native_tool_spec(DistributionHost::MacosArm64, catalog.compiler_tool())?);
    let record = helper_for_install(&catalog, RustPrepareDomain::Tools, &argv, "0.1.0")?;
    assert_eq!(catalog.compiler_tool(), PinnedTool::Rust);
    assert_eq!(catalog.rust_host().target_triple(), "aarch64-apple-darwin");
    assert_eq!(
        record.invocation().descriptor().operation(),
        SourceBoundOperation::RustPrepareReleaseMac
    );
    assert!(!record.source().contains("VELNOR_WRAPPER_VERIFY"));
    assert!(!record.source().contains("mr_boxington=true"));
    assert_eq!(install_argv(record.invocation(), "0.1.0"), Some(argv));
    Ok(())
}

#[test]
fn qualified_mise_precedes_cold_reset_and_all_rust_execution() -> Result<(), MiseError> {
    for role in [
        RustCompilerRole::RootLinux,
        RustCompilerRole::DesktopMac,
        RustCompilerRole::DesktopSourceMac,
        RustCompilerRole::ReleaseMac,
    ] {
        let catalog = catalog_for_role(role)?;
        let record = helper_for_install(
            &catalog,
            RustPrepareDomain::Tools,
            &install(&catalog)?,
            "0.1.0",
        )?;
        let source = record.source();
        let qualified = source.find("sha='").ok_or_else(invalid)?;
        let cold = source.find("rust_cold_prepare").ok_or_else(invalid)?;
        let bootstrap = source
            .find("https://static.rust-lang.org/rustup/archive")
            .ok_or_else(invalid)?;
        let manager = source
            .find("\"$manager\" set auto-self-update disable")
            .ok_or_else(invalid)?;
        let reshimmer = source.find("reshim --force").ok_or_else(invalid)?;
        let installer = source.find("install \"$@\"").ok_or_else(invalid)?;
        assert!(
            qualified < cold
                && cold < bootstrap
                && bootstrap < manager
                && manager < reshimmer
                && reshimmer < installer
        );
    }
    Ok(())
}

#[test]
fn foreign_compiler_unknown_duplicate_and_injected_selectors_rejected() -> Result<(), MiseError> {
    let root = ToolCatalog::pinned();
    let desktop = catalog_for_role(RustCompilerRole::DesktopMac)?;
    let good = install(&root)?;
    for extra in [
        desktop.tool_spec(PinnedTool::RustDesktop)?,
        "rust@latest".to_owned(),
        root.tool_spec(PinnedTool::MrBoxington)?,
        "$(touch sentinel)".to_owned(),
    ] {
        let mut argv = good.clone();
        argv.push(extra);
        assert!(helper_for_install(&root, RustPrepareDomain::Tools, &argv, "0.1.0").is_err());
    }
    let mut absent = good;
    absent.remove(install_prefix().len());
    assert!(helper_for_install(&root, RustPrepareDomain::Tools, &absent, "0.1.0").is_err());
    let mut missing_mbx = install(&desktop)?;
    let mbx = desktop.tool_spec(PinnedTool::MrBoxington)?;
    missing_mbx.retain(|arg| arg != &mbx);
    assert!(helper_for_install(&desktop, RustPrepareDomain::Tools, &missing_mbx, "0.1.0").is_err());
    Ok(())
}

#[test]
fn decoder_rejects_rebound_role_args_metadata_and_digest() -> Result<(), MiseError> {
    let catalog = ToolCatalog::pinned();
    let good = helper_for_install(
        &catalog,
        RustPrepareDomain::Tools,
        &install(&catalog)?,
        "0.1.0",
    )?;
    let invocation = good.invocation();
    let wrong_role = SourceBoundHelper::compiled(
        SourceBoundOperation::RustPrepareDesktopMac,
        SourceBoundOperation::RustPrepareDesktopMac.path(),
        invocation.descriptor().source_sha256(),
    )
    .map_err(|error| contract(&error))?;
    let wrong_digest = SourceBoundHelper::compiled(
        SourceBoundOperation::RustPrepareRootLinux,
        SourceBoundOperation::RustPrepareRootLinux.path(),
        &"0".repeat(64),
    )
    .map_err(|error| contract(&error))?;
    for descriptor in [wrong_role, wrong_digest] {
        let changed = HelperInvocation::compiled(
            descriptor,
            invocation.args().to_vec(),
            invocation.installed_selectors().to_vec(),
        )
        .map_err(|error| contract(&error))?;
        assert!(install_argv(&changed, "0.1.0").is_none());
    }
    let metadata = HelperInvocation::compiled(
        invocation.descriptor().clone(),
        invocation.args().to_vec(),
        vec!["rust@latest".to_owned()],
    )
    .map_err(|error| contract(&error))?;
    assert!(install_argv(&metadata, "0.1.0").is_none());
    let mut args = invocation.args().to_vec();
    args[0] = "/tmp/untrusted/mise".to_owned();
    let rebound = HelperInvocation::compiled(
        invocation.descriptor().clone(),
        args,
        invocation.installed_selectors().to_vec(),
    )
    .map_err(|error| contract(&error))?;
    assert!(install_argv(&rebound, "0.1.0").is_none());
    Ok(())
}

#[test]
fn repair_and_payload_validation_precede_wrapper_generation_and_execution() -> Result<(), MiseError>
{
    let catalog = catalog_for_role(RustCompilerRole::DesktopMac)?;
    let record = helper_for_install(
        &catalog,
        RustPrepareDomain::PlanningBootstrap,
        &install(&catalog)?,
        "0.1.0",
    )?;
    let source = record.source();
    let repair = source.find("reshim --force").ok_or_else(invalid)?;
    let install = source.find("install \"$@\"").ok_or_else(invalid)?;
    let inventory = source.rfind("printf '%s:%s").ok_or_else(invalid)?;
    let wrapper = source.find(" exec '").ok_or_else(invalid)?;
    assert!(repair < install && install < inventory && inventory < wrapper);
    assert!(source.contains("sha256(wrapper.read_bytes())"));
    assert!(source.contains("foreign cargo wrapper"));
    assert!(source.contains("MISE_OFFLINE=true"));
    assert!(!source.contains("eval "));
    Ok(())
}

#[test]
fn reconstruct_record_rejects_environment_rebinding() -> Result<(), MiseError> {
    let catalog = ToolCatalog::pinned();
    let record = helper_for_install(
        &catalog,
        RustPrepareDomain::PlanningBootstrap,
        &install(&catalog)?,
        "0.1.0",
    )?;
    for (key, value) in [
        (
            "MISE_DATA_DIR",
            crate::runtime_paths::PLANNING_MISE_DATA_DIR,
        ),
        ("CARGO_HOME", "/tmp/foreign"),
        ("VELNOR_TOOL_CACHE_IDENTITY", "toolset@foreign"),
        ("BASH_ENV", "/tmp/foreign"),
    ] {
        let mut env = record.environment().clone();
        env.insert(key.to_owned(), value.to_owned());
        assert!(record_for_invocation(record.invocation(), &env, "0.1.0").is_err());
    }
    Ok(())
}

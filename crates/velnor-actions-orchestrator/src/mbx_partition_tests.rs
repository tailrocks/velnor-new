//! Data fixtures exercise compatibility shape, never installation authority.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

fn fixture() -> Result<ToolContext, OrchestratorError> {
    let catalog = ToolCatalog::pinned();
    let selector = catalog
        .tool_spec(catalog.compiler_tool())
        .map_err(owner_error)?;
    let mut selectors = vec![
        selector.clone(),
        catalog
            .tool_spec(PinnedTool::MrBoxington)
            .map_err(owner_error)?,
    ];
    selectors.sort();
    Ok(ToolContext {
        schema: 1,
        descriptor: ToolCacheDescriptor {
            domain: ToolCacheDomain::Full,
            target: "x86_64-unknown-linux-gnu".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            selectors,
            immutable_identity: "fixture-tools-v1".to_owned(),
            qualification_identity: format!("qualified-tools@b3-{}", "1".repeat(64)),
        },
        rust: RustContext {
            toolchain: catalog.rustup_toolchain(),
            selector,
            host: catalog.rust_host().target_triple().to_owned(),
            components: catalog.rust_install_options().components().to_vec(),
            targets: vec![catalog.rust_host().target_triple().to_owned()],
        },
        mbx: MbxContext {
            qualification: format!("b3-{}", "2".repeat(64)),
            source_repository: "https://github.com/jdx/mr-boxington".to_owned(),
            source_commit: "3".repeat(40),
            source_tree: "4".repeat(40),
            binary_sha256: "5".repeat(64),
            abi: "fixture-mbx-abi-v1".to_owned(),
        },
    })
}

#[test]
fn partition_has_no_invocation_provenance_or_runtime_expressions() -> Result<(), OrchestratorError>
{
    let context = fixture()?;
    context.descriptor.validate()?;
    let canonical = context.canonical()?;
    let fields: serde_json::Value =
        serde_json::from_str(&canonical).map_err(|error| invalid(&error.to_string()))?;
    assert_eq!(
        fields
            .as_object()
            .map(|object| object.keys().cloned().collect::<Vec<_>>()),
        Some(vec![
            "descriptor".to_owned(),
            "mbx".to_owned(),
            "rust".to_owned(),
            "schema".to_owned()
        ]),
    );
    assert!(!canonical.contains("${{"));
    assert!(!canonical.contains("run_id"));
    assert!(!canonical.contains("run_attempt"));
    assert!(!canonical.contains("checkout_commit"));
    assert_eq!(canonical, context.clone().canonical()?);
    Ok(())
}

#[test]
fn compiler_and_owned_binary_compatibility_changes_always_repartition()
-> Result<(), OrchestratorError> {
    let base = fixture()?;
    let edits: [fn(&mut ToolContext); 9] = [
        |context| context.rust.toolchain.push_str("-changed"),
        |context| context.rust.components.push("rust-src".to_owned()),
        |context| {
            context
                .rust
                .targets
                .push("wasm32-unknown-unknown".to_owned())
        },
        |context| context.mbx.qualification = format!("b3-{}", "6".repeat(64)),
        |context| context.mbx.source_commit = "7".repeat(40),
        |context| context.mbx.source_tree = "8".repeat(40),
        |context| context.mbx.binary_sha256 = "9".repeat(64),
        |context| context.mbx.abi.push_str("-changed"),
        |context| {
            context.descriptor.qualification_identity =
                format!("qualified-tools@b3-{}", "a".repeat(64))
        },
    ];
    let mut identities = BTreeSet::from([base.canonical()?]);
    for edit in edits {
        let mut changed = base.clone();
        edit(&mut changed);
        assert!(identities.insert(changed.canonical()?));
    }
    assert_eq!(identities.len(), edits.len() + 1);
    Ok(())
}

#[test]
fn unchecked_setup_cannot_supply_tool_partition_authority() -> Result<(), OrchestratorError> {
    let descriptor = fixture()?.descriptor;
    let missing = MiseSetup {
        version: "2026.10.0".to_owned(),
        bootstraps: BTreeMap::new(),
    };
    let error = validate_setup(&missing, &descriptor, DistributionHost::LinuxAmd64, "0.1.0")
        .expect_err("empty bootstrap must fail before qualification");
    assert!(
        error
            .to_string()
            .contains("mise_bootstrap_authority_missing")
    );
    assert!(host_for_target("ambient-host").is_err());
    Ok(())
}

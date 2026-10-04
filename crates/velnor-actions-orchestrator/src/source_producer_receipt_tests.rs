//! Pure source-owner reconstruction; no distribution or download proof is claimed.

use std::collections::BTreeMap;

use velnor_actions_contract::{
    CompiledSourceHelper, HelperInvocation, SourceBoundHelper, SourceBoundOperation,
    SourceProducer, SourceProducerRole, StepId, ToolCacheDescriptor, ToolCacheDomain,
    ToolProducerSelection,
};
use velnor_actions_mise::{PinnedTool, ToolCatalog};

use super::super::{descriptor::RustSourceDescriptor, source};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn fixture_json() -> String {
    let manifest = "[package]\nname='demo'\nversion='1.0.0'\n[dependencies]\nz='1'\n";
    let checksum = "a".repeat(64);
    let lock = format!(
        "version=4\n[[package]]\nname='demo'\nversion='1.0.0'\n\
         [[package]]\nname='z'\nversion='1.0.0'\n\
         source='registry+https://github.com/rust-lang/crates.io-index'\n\
         checksum='{checksum}'\n"
    );
    format!(
        "{{\"schema\":1,\"rust_version\":\"{}\",\"target\":\"{}\",\
         \"roots\":[\"\"],\"manifests\":[[\"Cargo.toml\",{}]],\
         \"locks\":[[\"\",{}]],\"archives\":[[\"z\",\"1.0.0\",\"{checksum}\"]],\
         \"mode\":\"complete-locked-workspace\"}}",
        velnor_actions_mise::catalog::RUST_VERSION,
        velnor_actions_mise::catalog::RUST_TARGET_TRIPLE,
        serde_json::to_string(manifest).expect("manifest JSON"),
        serde_json::to_string(&lock).expect("lock JSON")
    )
}

fn fixture() -> (CompiledSourceHelper, SourceProducer) {
    let descriptor = RustSourceDescriptor::from_hex(&hex(fixture_json().as_bytes()))
        .expect("canonical public captured closure");
    let original = source::compiled_helper(&descriptor, env!("CARGO_PKG_VERSION"))
        .expect("actual compiled source owner");
    let identity =
        super::super::descriptor::source_identity(&descriptor, &original).expect("source identity");
    // Shape-only bootstrap metadata. Receipt reconstruction does not qualify tools.
    let tool = ToolCacheDescriptor {
        domain: ToolCacheDomain::Full,
        target: velnor_actions_mise::catalog::RUST_TARGET_TRIPLE.to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
        selectors: vec![
            ToolCatalog::pinned()
                .tool_spec(PinnedTool::Rust)
                .expect("canonical Rust selector"),
        ],
        immutable_identity: "receipt-shape-only".to_owned(),
        qualification_identity: format!("qualified-tools@b3-{}", "0".repeat(64)),
    };
    let metadata = SourceProducer {
        role: SourceProducerRole::Cargo,
        selection: ToolProducerSelection {
            cargo_fallback: true,
            ..ToolProducerSelection::default()
        },
        tool_cache: Some(tool),
        source_identity: identity,
        verification_step: StepId::new("velnor-rust-source-verify").expect("id"),
        restore_step: StepId::new("velnor-sources-cache").expect("id"),
        save_step: StepId::new("velnor-rust-source-save").expect("id"),
        publication_step: StepId::new("velnor-rust-source-publication").expect("id"),
        report_step: StepId::new("velnor-source-report").expect("id"),
    };
    metadata.validate().expect("metadata shape");
    (original, metadata)
}

fn invocation_with_args(original: &CompiledSourceHelper, args: Vec<String>) -> HelperInvocation {
    HelperInvocation::compiled(original.invocation().descriptor().clone(), args, Vec::new())
        .expect("invocation shape")
}

fn reject_receipt_and_projection(
    invocation: &HelperInvocation,
    metadata: &SourceProducer,
    version: &str,
) {
    assert!(super::record_for_receipt(invocation, metadata, version).is_err());
    assert!(super::super::source_compatibility_projection(invocation, metadata, version).is_err());
}

#[test]
fn actual_owner_reconstructs_exact_record_and_owned_identity_environment() {
    let (original, metadata) = fixture();
    let expected = original.clone().with_environment(BTreeMap::from([(
        "VELNOR_SOURCE_IDENTITY".to_owned(),
        metadata.source_identity.clone(),
    )]));
    let reconstructed =
        super::record_for_receipt(original.invocation(), &metadata, env!("CARGO_PKG_VERSION"))
            .expect("actual owner reconstruction");
    assert_eq!(reconstructed, expected);
    assert!(
        reconstructed
            .source()
            .contains("export PATH=/usr/bin:/bin:/usr/sbin:/sbin")
    );
    assert!(
        reconstructed
            .source()
            .contains("/usr/bin/python3 -I -S -B - \"$1\"")
    );
    assert!(reconstructed.invocation().installed_selectors().is_empty());
    assert!(reconstructed.invocation().execution_prefix().is_empty());
    assert!(reconstructed.execution_recipe().is_none());
    assert!(!reconstructed.github_output());
}

#[test]
fn actual_owner_projection_has_exact_canonical_bytes_and_domain_digest() {
    let (original, metadata) = fixture();
    let projection = super::super::source_compatibility_projection(
        original.invocation(),
        &metadata,
        env!("CARGO_PKG_VERSION"),
    )
    .expect("actual owner projection");
    let expected = fixture_json().replace(
        &format!(
            "\"rust_version\":\"{}\",",
            velnor_actions_mise::catalog::RUST_VERSION
        ),
        "",
    );
    assert_eq!(projection.canonical_bytes(), expected.as_bytes());
    let mut evidence = b"velnor-rust-source-projection-v1\0".to_vec();
    evidence.extend_from_slice(expected.as_bytes());
    assert_eq!(
        projection.digest(),
        velnor_actions_contract::digest_b3(&evidence)
    );
    assert_ne!(
        projection.digest(),
        velnor_actions_contract::digest_b3(expected.as_bytes())
    );
    assert_ne!(projection.digest(), metadata.source_identity);
    assert_eq!(
        projection,
        super::super::source_compatibility_projection(
            original.invocation(),
            &metadata,
            env!("CARGO_PKG_VERSION"),
        )
        .expect("deterministic projection")
    );
}

#[test]
fn generated_marker_and_self_consistent_digest_do_not_grant_source_ownership() {
    let (original, metadata) = fixture();
    let forged_source = velnor_actions_contract::generated_source(
        env!("CARGO_PKG_VERSION"),
        "set -eu\nprintf forged\n",
    )
    .expect("generated marker");
    let operation = SourceBoundOperation::RustSourceProducer;
    let owner = SourceBoundHelper::compiled(
        operation,
        operation.path(),
        &velnor_actions_contract::compiled_source_sha256(forged_source.as_bytes()),
    )
    .expect("descriptor shape");
    let invocation =
        HelperInvocation::compiled(owner, original.invocation().args().to_vec(), vec![])
            .expect("invocation shape");
    let forged = CompiledSourceHelper::compiled(invocation, forged_source)
        .expect("self-consistent unowned record");
    reject_receipt_and_projection(forged.invocation(), &metadata, env!("CARGO_PKG_VERSION"));
}

#[test]
fn receipt_rejects_noncanonical_or_unqualified_captured_descriptors() {
    let (original, metadata) = fixture();
    let json = fixture_json();
    let changed = [
        format!(" {json}"),
        json.replacen('{', "{\"unknown\":true,", 1),
        json.replace(
            "complete-locked-workspace",
            "native-tree-selected-containing",
        ),
        json.replace(velnor_actions_mise::catalog::RUST_VERSION, "1.97.0"),
        json.replace(
            "registry+https://github.com/rust-lang/crates.io-index",
            "registry+https://private.example/index",
        ),
        json.replace(
            "registry+https://github.com/rust-lang/crates.io-index",
            "git+https://private.example/repository",
        ),
        json.replace(&"a".repeat(64), &"b".repeat(64)).replacen(
            &format!("\"{}\"", "b".repeat(64)),
            &format!("\"{}\"", "a".repeat(64)),
            1,
        ),
    ];
    for changed_json in changed {
        let invocation = invocation_with_args(&original, vec![hex(changed_json.as_bytes())]);
        reject_receipt_and_projection(&invocation, &metadata, env!("CARGO_PKG_VERSION"));
    }
}

#[test]
fn receipt_rejects_extra_arguments_selectors_prefix_identity_and_version() {
    let (original, metadata) = fixture();
    let mut args = original.invocation().args().to_vec();
    args.push("extra".to_owned());
    let extra = invocation_with_args(&original, args);
    let installed = HelperInvocation::compiled(
        original.invocation().descriptor().clone(),
        original.invocation().args().to_vec(),
        metadata
            .tool_cache
            .as_ref()
            .expect("tools")
            .selectors
            .clone(),
    )
    .expect("selector shape");
    let mut encoded = serde_json::to_value(original.invocation()).expect("invocation JSON");
    encoded["execution_prefix"] = serde_json::json!(["env", "--"]);
    let prefixed: HelperInvocation = serde_json::from_value(encoded).expect("prefix wire");
    prefixed.validate().expect("prefix shape");
    for invocation in [extra, installed, prefixed] {
        reject_receipt_and_projection(&invocation, &metadata, env!("CARGO_PKG_VERSION"));
    }
    let mut changed = metadata.clone();
    changed.source_identity.push_str("-foreign");
    changed.validate().expect("foreign identity shape");
    reject_receipt_and_projection(original.invocation(), &changed, env!("CARGO_PKG_VERSION"));
    reject_receipt_and_projection(original.invocation(), &metadata, "different-version");
}

#[test]
fn receipt_rejects_missing_bootstrap_and_foreign_role_or_target() {
    let (original, metadata) = fixture();
    let mut missing = metadata.clone();
    missing.tool_cache = None;
    let mut foreign_role = missing.clone();
    foreign_role.role = SourceProducerRole::Npm;
    foreign_role.selection.cargo_fallback = false;
    let mut foreign_target = metadata;
    let tool = foreign_target.tool_cache.as_mut().expect("tools");
    tool.target = "aarch64-apple-darwin".to_owned();
    tool.runs_on = "macos-15".to_owned();
    for changed in [missing, foreign_role, foreign_target] {
        changed.validate().expect("metadata shape");
        reject_receipt_and_projection(original.invocation(), &changed, env!("CARGO_PKG_VERSION"));
    }
}

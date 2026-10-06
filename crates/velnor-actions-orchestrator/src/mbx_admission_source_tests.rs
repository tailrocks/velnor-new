use super::*;
use velnor_actions_contract::{
    MbxCacheDomain, MbxExportDescriptor, MbxOwnerIdentity, StepId, ToolProducerSelection,
};

fn metadata() -> PureMbxProducer {
    let task = "stack/rust/default/check/default".to_owned();
    PureMbxProducer {
        descriptor: MbxExportDescriptor {
            domain: MbxCacheDomain::Validation,
            producer_job_id: "rust-owner".into(),
            runs_on: "ubuntu-26.04".into(),
            target: "x86_64-unknown-linux-gnu".into(),
            workspace_roots: vec![".".into()],
            configuration_digest: velnor_actions_contract::digest_b3(b"configuration"),
            task_digests: [(task.clone(), velnor_actions_contract::digest_b3(b"task"))].into(),
            owner: MbxOwnerIdentity {
                version: "1.21.1-owned-cache-transport".into(),
                binary_sha256: "a".repeat(64),
                qualification_identity: "c".repeat(64),
                source_sha: "b".repeat(40),
            },
            action_sha: "c".repeat(40),
        },
        selection: ToolProducerSelection {
            tasks: vec![task],
            cargo_fallback: false,
            unconditional: false,
        },
        installation_step: StepId::new("mbx-install").expect("step"),
        admission_step: StepId::new("mbx-admit").expect("step"),
        verification_step: StepId::new("mbx-verify").expect("step"),
        save_step: StepId::new("mbx-save").expect("step"),
        publication_step: StepId::new("mbx-publication").expect("step"),
        report_step: StepId::new("mbx-report").expect("step"),
    }
}

fn sources() -> (MbxProducerSources, ToolCatalog, MiseSetup) {
    let catalog = ToolCatalog::pinned();
    let setup = crate::test_mise::setup("2026.9.16", &"a".repeat(64));
    let sources = compiled_mbx_sources(&metadata(), &catalog, &setup, env!("CARGO_PKG_VERSION"))
        .expect("honest unsupported draft");
    (sources, catalog, setup)
}

#[test]
fn missing_transport_publication_is_explicitly_cold_and_has_no_credentials() {
    let (sources, catalog, setup) = sources();
    sources
        .validate(&metadata(), &catalog, &setup, env!("CARGO_PKG_VERSION"))
        .expect("exact reconstruction");
    let MbxCapturedSourceInputs::UnsupportedPublication {
        owner_unavailable,
        action_unavailable,
        action,
    } = &sources.inputs;
    assert!(
        owner_unavailable.is_some(),
        "actual registry record is absent"
    );
    assert!(action_unavailable.as_ref().is_some_and(|reason| {
        reason.contains("comparison export action qualification absent")
    }));
    assert!(action.is_none());
    assert!(sources.records[3].source().contains("sourceidentity="));
    assert!(!sources.records[3].source().contains("source_identity="));
    for record in sources.source_helpers() {
        assert!(record.environment().is_empty());
        assert!(record.invocation().installed_selectors().is_empty());
        assert!(!record.source().contains("=true"));
        assert!(!record.source().contains("GH_TOKEN"));
        assert!(!record.source().contains("curl"));
    }
}

#[test]
fn changed_descriptor_or_argument_or_environment_cannot_keep_source_authority() {
    let (mut sources, catalog, setup) = sources();
    let mut changed = metadata();
    changed.descriptor.action_sha = "d".repeat(40);
    assert!(
        sources
            .validate(&changed, &catalog, &setup, env!("CARGO_PKG_VERSION"))
            .is_err()
    );
    let expected = sources.records[1].clone();
    let invocation = HelperInvocation::compiled(
        expected.invocation().descriptor().clone(),
        vec!["arbitrary caller input".into()],
        Vec::new(),
    )
    .expect("shape alone");
    sources.records[1] = CompiledSourceHelper::compiled(invocation, expected.source().into())
        .expect("correctly hashed caller arguments");
    assert!(
        sources
            .validate(&metadata(), &catalog, &setup, env!("CARGO_PKG_VERSION"))
            .is_err()
    );
    sources.records[1] = expected.with_environment([("GH_TOKEN".into(), "injected".into())].into());
    assert!(
        sources
            .validate(&metadata(), &catalog, &setup, env!("CARGO_PKG_VERSION"))
            .is_err()
    );
}

#[test]
fn correctly_hashed_arbitrary_body_cannot_replace_owner_computation() {
    let (mut sources, catalog, setup) = sources();
    sources.records[1] = record(
        SourceBoundOperation::MbxArtifactAdmission,
        "printf 'admitted=true\\n' >> \"$GITHUB_OUTPUT\"\n",
        "caller-shaped-identity",
        env!("CARGO_PKG_VERSION"),
    )
    .expect("generic helper shape");
    sources.records[1]
        .validate_binding()
        .expect("matching body hash");
    assert!(
        sources
            .validate(&metadata(), &catalog, &setup, env!("CARGO_PKG_VERSION"))
            .is_err()
    );
}

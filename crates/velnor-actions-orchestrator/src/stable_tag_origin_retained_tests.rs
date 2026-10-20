use super::*;
use serde_json::json;
use velnor_actions_contract::{
    CargoVersionBinding, CargoVersionPolicy, PACKAGE_RELEASE_POLICY_SCHEMA_V1, PackageReleasePolicy,
};

fn proof() -> StableOriginProof {
    let (plan, mut expected, reference) = super::super::tests::fixture();
    let policy = PackageReleasePolicy {
        schema: PACKAGE_RELEASE_POLICY_SCHEMA_V1.into(),
        channel: plan.channel,
        repository: plan.repository.clone(),
        default_branch: plan.default_branch.clone(),
        products: plan.products.clone(),
        companions: plan.companions.clone(),
        version: CargoVersionPolicy {
            version_package: "app".into(),
            bindings: vec![CargoVersionBinding {
                package: "app".into(),
                manifest_path: "Cargo.toml".into(),
                workspace_manifest_path: "Cargo.toml".into(),
            }],
        },
    };
    expected.policy_sha256 =
        super::super::sha256(&super::super::canonical(&policy).expect("policy"));
    let tag = super::super::Tag {
        name: "v1.2.3".into(),
        object_type: "tag".into(),
        object_sha: "b".repeat(40),
        target_sha: expected.context.source_sha.clone(),
        message: super::super::tag::authorization_message(&expected).expect("message"),
    };
    let intent = Intent {
        schema: "velnor.stable-tag-intent.v1".into(),
        authorization: expected.clone(),
        tag: tag.clone(),
    };
    let origin = Origin {
        schema: "velnor.stable-tag-origin.v1".into(),
        intent: reference.clone(),
        authorization: expected.clone(),
        completion_context: expected.context.clone(),
        tag,
    };
    let receipt = serde_json::from_value(json!({
        "schema":"velnor.package-release.v1","channel":"stable","repository":{"id":7,"slug":"example/app"},
        "source":{"commit_sha":"a".repeat(40),"ref":"refs/heads/main","default_branch":"main","event":"workflow_dispatch","ci_run_id":9,"ci_run_attempt":1},
        "workflow":{"path":".github/workflows/binary-release.yml","ref":"example/app/.github/workflows/binary-release.yml@refs/heads/main","authority_sha":"a".repeat(40),"run_id":11,"run_attempt":1,"producer_run_id":11,"producer_run_attempt":1,"signer_job_id":"stable-signer","signer_workflow":".github/workflows/binary-release.yml"},
        "version":{"base":"1.2.3","value":"1.2.3","commit_count":null,"source_sha7":null},"tag":"v1.2.3","products":[],"companions":[],"assets":[],
        "evidence":{"profile":"signed-sbom-v1","sbom_format":"CycloneDX-JSON","signer_issuer":"https://token.actions.githubusercontent.com","signer_workflow_ref":"example/app/.github/workflows/binary-release.yml@refs/heads/main","attestation_predicate":"https://slsa.dev/provenance/v1","attested_subject_names":[]}
    })).expect("typed binding fixture");
    let mut proof = StableOriginProof {
        seal: String::new(),
        expected,
        policy,
        plan,
        receipt,
        origin_archive: reference,
        origin,
        intent,
    };
    proof.seal = proof.fingerprint().expect("seal");
    proof
}

#[test]
fn admitted_origin_seal_binds_all_immutable_authorization_and_artifact_identities() {
    let original = proof();
    original
        .verify_inputs(&original.plan, &original.receipt, &original.policy)
        .expect("complete typed binding");
    for mutate in [
        |p: &mut StableOriginProof| p.expected.context.run_attempt += 1,
        |p: &mut StableOriginProof| p.expected.context.ci_run_attempt += 1,
        |p: &mut StableOriginProof| p.expected.context.source_sha = "d".repeat(40),
        |p: &mut StableOriginProof| p.expected.version_package = "other".into(),
        |p: &mut StableOriginProof| p.origin.completion_context.run_attempt += 1,
        |p: &mut StableOriginProof| p.origin.authorization.context.run_id += 1,
        |p: &mut StableOriginProof| p.origin_archive.id += 1,
        |p: &mut StableOriginProof| p.origin_archive.sha256 = "e".repeat(64),
        |p: &mut StableOriginProof| p.origin.intent.id += 1,
        |p: &mut StableOriginProof| p.origin.intent.sha256 = "e".repeat(64),
        |p: &mut StableOriginProof| p.origin.tag.object_sha = "d".repeat(40),
        |p: &mut StableOriginProof| p.origin.tag.target_sha = "d".repeat(40),
        |p: &mut StableOriginProof| p.intent.tag.message.push('x'),
        |p: &mut StableOriginProof| p.policy.version.version_package = "other".into(),
        |p: &mut StableOriginProof| p.plan.base_version = "9.9.9".into(),
        |p: &mut StableOriginProof| p.receipt.repository.id += 1,
    ] {
        let mut changed = proof();
        mutate(&mut changed);
        assert!(
            changed
                .verify_inputs(&original.plan, &original.receipt, &original.policy)
                .is_err()
        );
    }
}

#[test]
fn cached_typed_origin_still_reads_live_exact_annotated_tag_and_rejects_copied_object() {
    let proof = proof();
    let reference = json!({"ref":"refs/tags/v1.2.3","object":{"type":"tag","sha":"d".repeat(40)}});
    let object = json!({"sha":"d".repeat(40),"tag":"v1.2.3","message":proof.origin.tag.message,
        "object":{"type":"commit","sha":"a".repeat(40)}});
    let (api, calls) = GitHubApi::with_fixture(
        "example/app",
        vec![
            (
                "repos/example/app/git/ref/tags/v1.2.3".into(),
                reference.clone(),
            ),
            (
                format!("repos/example/app/git/tags/{}", "d".repeat(40)),
                object,
            ),
            ("repos/example/app/git/ref/tags/v1.2.3".into(), reference),
        ],
    );
    assert!(proof.verify_tag(&api).is_err());
    calls.assert_drained();
    assert_eq!(
        calls.observed().len(),
        3,
        "cached origin never replaces mutable tag checks"
    );
}

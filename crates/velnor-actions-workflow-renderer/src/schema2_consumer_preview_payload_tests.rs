//! Literal payload graph and immutable artifact handoff regressions.

use super::*;
use velnor_actions_contract::preview_production::{
    PreviewProductionContext, expected_preview_payload_pairs, preview_payload_artifact_name,
    preview_payload_job_id,
};

fn fields(job: &Yaml) -> &[(String, Yaml)] {
    let Yaml::Map(fields) = job else {
        panic!("job map")
    };
    fields
}

fn field<'a>(job: &'a Yaml, key: &str) -> &'a Yaml {
    &fields(job)
        .iter()
        .find(|(name, _)| name == key)
        .expect(key)
        .1
}

#[test]
fn every_literal_pair_has_exact_owner_and_complete_assembly_dependencies() {
    let spec = fixture();
    let pairs = expected_preview_payload_pairs(&spec.plan).expect("pairs");
    assert_eq!(pairs.len(), 6);
    let jobs = production::jobs(&spec).expect("production");
    assert_eq!(jobs.len(), 7);
    let mut expected = vec![Yaml::str("preview-eligibility")];
    for ((id, job), pair) in jobs.iter().zip(&pairs) {
        assert_eq!(
            *id,
            preview_payload_job_id(&spec.plan, pair).expect("owner")
        );
        assert_eq!(field(job, "name"), &Yaml::str(id));
        assert_eq!(field(job, "timeout-minutes"), &Yaml::Int(5));
        assert_eq!(field(job, "needs"), &needs(&["preview-eligibility"]));
        assert_eq!(
            field(job, "permissions"),
            &permissions(&[("contents", "read")])
        );
        expected.push(Yaml::str(id));
        let rendered = crate::yaml::render_yaml(job);
        assert!(rendered.contains(&format!("VELNOR_PREVIEW_PRODUCT_ID: {}", pair.product_id)));
        assert!(rendered.contains(&format!("VELNOR_PREVIEW_TARGET: {}", pair.target)));
        assert!(rendered.contains("VELNOR_PREVIEW_STAGE: payload"));
        assert!(!rendered.contains("GH_TOKEN: \"${{ github.token }}\""));
        assert!(!rendered.contains("matrix:"));
    }
    let (id, assembly) = jobs.last().expect("assembly");
    assert_eq!(id, "preview-assemble");
    assert_eq!(field(assembly, "name"), &Yaml::str(id));
    assert_eq!(field(assembly, "needs"), &Yaml::Seq(expected));
    assert_eq!(
        field(assembly, "permissions"),
        &permissions(&[("actions", "read"), ("contents", "read")])
    );
    let rendered = crate::yaml::render_yaml(assembly);
    assert!(rendered.contains("VELNOR_PREVIEW_STAGE: assemble"));
    assert!(rendered.contains("${{ toJSON(needs) }}"));
    assert!(!rendered.contains("id-token:"));
    assert!(!rendered.contains("environment:"));
}

#[test]
fn generated_leaf_names_match_native_admission_without_name_downloads() {
    let spec = fixture();
    let context = PreviewProductionContext {
        repository_id: 42,
        head_repository_id: 42,
        source_sha: "a".repeat(40),
        source_ref: "refs/heads/main".to_owned(),
        workflow_authority_sha: "a".repeat(40),
        run_id: 123,
        run_attempt: 2,
        ci_run_id: 99,
        ci_run_attempt: 1,
        commit_count: 100,
        version: "1.2.3-preview.100+aaaaaaa".to_owned(),
    };
    for pair in expected_preview_payload_pairs(&spec.plan).expect("pairs") {
        let upload = production::upload(Some(&pair));
        let with = field(&upload, "with");
        let Yaml::Str(name) = field(with, "name") else {
            panic!("artifact name")
        };
        let actual = name
            .replace("${{ github.run_id }}", "123")
            .replace("${{ github.run_attempt }}", "2");
        assert_eq!(
            actual,
            preview_payload_artifact_name(&spec.plan, &pair, &context).expect("native name")
        );
        assert_eq!(field(&upload, "id"), &Yaml::str("upload"));
        assert_eq!(field(with, "overwrite"), &Yaml::Bool(false));
    }
    let rendered = render_consumer_preview(&spec).expect("render").bytes;
    let signer = job(&rendered, "preview-signer", Some("preview-publisher"));
    assert!(signer.contains("admit-preview-aggregate-v1"));
    assert_eq!(signer.matches("id: aggregate\n").count(), 1);
    assert_eq!(signer.matches("id: release\n").count(), 1);
    assert!(signer.contains("needs.preview-assemble.outputs.artifact_id"));
    assert!(signer.contains("needs.preview-assemble.outputs.artifact_digest"));
    assert!(!signer.contains("actions/download-artifact"));
    assert!(!rendered.contains("preview-producer:"));
}

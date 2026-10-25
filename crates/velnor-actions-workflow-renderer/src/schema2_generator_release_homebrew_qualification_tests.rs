use super::*;
use crate::yaml::render_yaml;

#[test]
fn hosted_homebrew_job_is_read_only_bounded_and_scoped_to_exact_candidate_artifacts() {
    let pins = crate::schema2::product_release_test_pins::test_pins();
    let mut actions = Vec::new();
    let (id, job) = job(&pins, &mut actions).expect("render Homebrew qualification");
    let job_yaml = render_yaml(&job);
    let action_yaml = render_yaml(&actions[0].1);
    let Yaml::Map(job_fields) = &job else {
        panic!("qualification job is a mapping");
    };

    assert_eq!(id, "qualify-homebrew-macos");
    assert_eq!(
        actions[0].0,
        ".github/actions/generator-release-qualify-homebrew/action.yml"
    );
    assert!(job_yaml.contains("timeout-minutes: 30"));
    assert!(job_yaml.contains("contents: read"));
    assert!(job_yaml.contains("attestations: read"));
    assert!(!job_yaml.contains("contents: write"));
    assert!(!job_yaml.contains("id-token: write"));
    assert!(matches!(
        job_fields.iter().find(|(key, _)| key == "needs"),
        Some((_, Yaml::Seq(needs)))
            if needs == &[Yaml::str("build-macos"), Yaml::str("candidate-manifest")]
    ));

    assert!(action_yaml.contains("${{ inputs.artifact_id }}"));
    assert!(action_yaml.contains("${{ inputs.manifest_artifact_id }}"));
    assert!(action_yaml.contains("${{ inputs.manifest_sha256 }}"));
    let product = super::super::assets::MACOS_ARM64;
    let artifact_download = action_yaml
        .find("Download exact ARM candidate artifact")
        .expect("candidate artifact download step is rendered");
    let extraction = action_yaml
        .find("Prevalidate and extract uploaded candidate archive")
        .expect("archive extraction step is rendered");
    let manifest_download = action_yaml
        .find("Download canonical same-run candidate manifest")
        .expect("manifest download step is rendered");
    let provenance = action_yaml
        .find("Verify source-bound build record")
        .expect("provenance verification step is rendered");
    assert!(
        artifact_download < extraction
            && extraction < manifest_download
            && manifest_download < provenance,
        "download and extract candidate before provenance validation"
    );
    assert!(action_yaml.contains("scripts/with-owned-archive-guard.sh --"));
    assert!(action_yaml.contains(&format!(
        "extract-candidate '{}/{}' '{}' '{}' '{}' '{}'",
        product.directory,
        product.archive,
        product.directory,
        product.binary,
        product.sidecar,
        product.provenance
    )));
    assert!(action_yaml.contains("GH_TOKEN: ${{ github.token }}"));
    assert!(action_yaml.contains("VELNOR_INTERNAL_OP: qualify-homebrew-v1"));
    assert!(!action_yaml.contains("homebrew-qualify"));
    assert!(action_yaml.contains("velnor-actions-"));
}

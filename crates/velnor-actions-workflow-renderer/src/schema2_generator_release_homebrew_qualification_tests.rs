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
    assert!(action_yaml.contains("GH_TOKEN: ${{ github.token }}"));
    assert!(action_yaml.contains("VELNOR_INTERNAL_OP: qualify-homebrew-v1"));
    assert!(!action_yaml.contains("homebrew-qualify"));
    assert!(action_yaml.contains("velnor-actions-"));
}

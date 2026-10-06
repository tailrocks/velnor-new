use super::{
    GradleArtifactHome, PROOF, SANITIZE, artifact_descriptors, proof_step, sanitizer_step,
    sanitizer_wrapper,
};
use std::{fs, process::Command};
use velnor_actions_contract::{Step, StepKind};

fn env_value<'a>(step: &'a Step, name: &str) -> Option<&'a str> {
    match &step.kind {
        StepKind::Shell { env, .. } => env.get(name).map(String::as_str),
        _ => None,
    }
}

#[test]
fn descriptors_are_the_reviewed_consumer_compile_closure() {
    let descriptors = artifact_descriptors();
    assert_eq!(descriptors.len(), 2);
    assert_eq!(descriptors[0].group, "io.micronaut");
    assert_eq!(descriptors[0].module, "micronaut-core");
    assert_eq!(descriptors[0].version, "4.10.14");
    assert_eq!(descriptors[1].group, "org.slf4j");
    assert_eq!(descriptors[1].module, "slf4j-api");
    assert_eq!(descriptors[1].version, "2.0.17");
    assert!(PROOF.contains("repo.maven.apache.org"));
    assert!(!PROOF.contains("source[\"url\"]"));
}

#[test]
fn proof_and_sanitizer_steps_use_fixed_ids_and_manifest_outputs() {
    let proof = proof_step(GradleArtifactHome::Consumer).expect("proof step");
    assert_eq!(
        env_value(&proof, "GRADLE_USER_HOME"),
        Some("${{ runner.temp }}/velnor/native/gradle")
    );
    assert_eq!(
        env_value(&proof, "VELNOR_GRADLE_ARTIFACT_HOME_ROLE"),
        Some("consumer")
    );
    assert_eq!(
        proof.id.as_ref().map(|id| id.as_str()),
        Some("velnor-gradle-public-proof-consumer")
    );
    let sanitizer = sanitizer_step(GradleArtifactHome::Consumer).expect("sanitizer step");
    assert_eq!(
        sanitizer.id.as_ref().map(|id| id.as_str()),
        Some("velnor-gradle-artifacts-sanitize-consumer")
    );
    assert_eq!(
        env_value(&sanitizer, "VELNOR_GRADLE_PUBLIC_PROOF_SAFE"),
        Some("${{steps.velnor-gradle-public-proof-consumer.outputs.proof-safe}}")
    );
    let producer = proof_step(GradleArtifactHome::Producer).expect("producer proof step");
    assert_eq!(
        env_value(&producer, "GRADLE_USER_HOME"),
        Some("${{ runner.temp }}/velnor/native/gradle-producer/gradle-home")
    );
    assert_eq!(
        env_value(&producer, "VELNOR_GRADLE_ARTIFACT_HOME_ROLE"),
        Some("producer")
    );
    assert_eq!(
        producer.id.as_ref().map(|id| id.as_str()),
        Some("velnor-gradle-public-proof-producer")
    );
    let producer_sanitizer =
        sanitizer_step(GradleArtifactHome::Producer).expect("producer sanitizer step");
    assert_eq!(
        producer_sanitizer.id.as_ref().map(|id| id.as_str()),
        Some("velnor-gradle-artifacts-sanitize-producer")
    );
    assert_eq!(
        env_value(&producer_sanitizer, "VELNOR_GRADLE_PUBLIC_PROOF_SAFE"),
        Some("${{steps.velnor-gradle-public-proof-producer.outputs.proof-safe}}")
    );
    assert!(SANITIZE.contains("velnor-public-artifacts.json"));
    assert!(SANITIZE.contains("sha256"));
}

#[test]
fn public_proof_fixture_rejects_arbitrary_coordinates() {
    let fixture = include_str!("workloads_cache_gradle_artifacts_fixture.py");
    assert!(
        Command::new("python3")
            .args(["-c", fixture, PROOF])
            .status()
            .expect("offline Gradle proof fixture")
            .success()
    );
}

#[test]
fn sanitizer_fixture_removes_private_and_rejects_forged_jar() {
    let fixture = include_str!("workloads_cache_gradle_artifacts_sanitize_fixture.py");
    assert!(
        Command::new("python3")
            .args(["-c", fixture, SANITIZE])
            .status()
            .expect("offline Gradle sanitizer fixture")
            .success()
    );
}

#[test]
fn sanitizer_failure_is_optional_and_reports_false() {
    let temp = tempfile::tempdir().expect("temp");
    let output = temp.path().join("github-output");
    let result = Command::new("sh")
        .args([
            "-e",
            "-c",
            &sanitizer_wrapper(GradleArtifactHome::Consumer).expect("wrapper"),
        ])
        .env("RUNNER_TEMP", temp.path())
        .env("GRADLE_USER_HOME", temp.path().join("outside"))
        .env("VELNOR_GRADLE_PUBLIC_PROOF_SAFE", "false")
        .env("GITHUB_OUTPUT", &output)
        .status()
        .expect("optional sanitizer");
    assert!(result.success());
    assert_eq!(
        fs::read_to_string(output).expect("sanitizer output"),
        "cache-safe=false\n"
    );
}

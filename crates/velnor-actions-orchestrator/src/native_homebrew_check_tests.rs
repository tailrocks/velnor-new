use super::tree::git_value;
use super::*;

#[cfg(test)]
#[test]
fn seed_identity_keeps_transition_custody_disjoint_from_source_authority() {
    let seed = NativeCheckAuthorityIdentity::Seed {
        transition_id: "jackin-homebrew-seed-921858f1".to_owned(),
        transition_digest: format!("b3-{}", "1".repeat(64)),
        base_tree: "a".repeat(40),
        sdk_source_sha: "b".repeat(40),
        sdk_version: "0.1.53".to_owned(),
        sdk_manifest_sha256: "c".repeat(64),
        sdk_seed_runner_binary_sha256: "d".repeat(64),
    };
    let value = serde_json::to_value(seed.clone()).expect("serialize seed authority");
    assert_eq!(
        value.get("kind").and_then(serde_json::Value::as_str),
        Some("seed")
    );
    assert!(value.get("source_repository_id").is_none());
    assert!(value.get("authority_sha").is_none());
    assert!(value.get("source_plan_digest").is_none());
    assert!(value.get("locator_blob_sha").is_none());
    assert!(value.get("formula_spec_digest").is_none());
    let decoded: NativeCheckAuthorityIdentity =
        serde_json::from_value(value.clone()).expect("decode seed identity");
    assert_eq!(decoded, seed);
    let first_digest = digest_b3(&canonical_json_bytes(&decoded).expect("canonical identity"));
    let second_digest = digest_b3(&canonical_json_bytes(&value).expect("canonical value"));
    assert_eq!(first_digest, second_digest);
}

#[cfg(test)]
#[test]
fn source_identity_binds_cutover_witness_without_seed_placeholders() {
    let cutover = crate::homebrew::structural_cutover::HomebrewSeedOutputBaseIdentity {
        schema: "homebrew-seed-output-base-v1".to_owned(),
        transition_id: "jackin-homebrew-seed-921858f1".to_owned(),
        transition_digest: format!("b3-{}", "1".repeat(64)),
        transition_base_sha: "a".repeat(40),
        transition_base_tree: "b".repeat(40),
        seed_output_base_sha: "c".repeat(40),
        seed_output_base_tree: "d".repeat(40),
        sdk_source_sha: "e".repeat(40),
        sdk_version: "0.1.53".to_owned(),
        sdk_manifest_sha256: "f".repeat(64),
        sdk_seed_runner_binary_sha256: "0".repeat(64),
        formula_spec_digest: format!("b3-{}", "2".repeat(64)),
        witness_digest: format!("b3-{}", "3".repeat(64)),
    };
    let source = |cutover: Option<
        crate::homebrew::structural_cutover::HomebrewSeedOutputBaseIdentity,
    >| NativeCheckAuthorityIdentity::Source {
        source_repository_id: 1_197_700_841,
        authority_sha: "4".repeat(40),
        source_plan_digest: format!("b3-{}", "5".repeat(64)),
        locator_blob_sha: "6".repeat(40),
        formula_spec_digest: format!("b3-{}", "7".repeat(64)),
        cutover: cutover.map(Box::new),
    };
    let without_cutover = source(None);
    let with_cutover = source(Some(cutover.clone()));
    let with_value = serde_json::to_value(&with_cutover).expect("serialize source identity");
    assert_eq!(
        with_value.get("kind").and_then(serde_json::Value::as_str),
        Some("source")
    );
    assert!(with_value.get("transition_id").is_none());
    assert_eq!(
        with_value
            .get("cutover")
            .and_then(|value| value.get("witness_digest"))
            .and_then(serde_json::Value::as_str),
        Some(cutover.witness_digest.as_str())
    );
    let without_digest = digest_b3(&canonical_json_bytes(&without_cutover).expect("canonical"));
    let with_digest = digest_b3(&canonical_json_bytes(&with_cutover).expect("canonical"));
    assert_ne!(without_digest, with_digest);
}

#[cfg(test)]
#[test]
fn native_homebrew_requests_round_trip_through_existing_plan_and_merge_parsers() {
    use std::time::{Duration, Instant};

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root");
    let deadline =
        velnor_actions_mise::CheckDeadline::from_start(Instant::now(), Duration::from_secs(30))
            .expect("deadline");
    let head = git_value(&root, vec!["HEAD".to_owned()], deadline).expect("checkout head");
    let temp = tempfile::tempdir().expect("temp directory");
    let plan_path = temp.path().join("plan-v1-request.json");
    crate::internal_request::write_request_parts(
        &plan_path,
        "local",
        "{}",
        Some(&head),
        None,
        temp.path(),
    )
    .expect("write plan request");
    let mut plan_request: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&plan_path).expect("read plan request"))
            .expect("plan request JSON");
    plan_request["run_key"] =
        serde_json::Value::String(velnor_actions_contract::run_key_for_ci(71, 1));
    plan_request["root"] = serde_json::Value::String(root.display().to_string());
    let (parsed, parsed_root) = crate::internal_request::checked_plan_request(
        &serde_json::to_string(&plan_request).expect("encode plan request"),
    )
    .expect("existing plan parser accepts generated request");
    assert_eq!(parsed.event, WorkflowEvent::Local);
    assert_eq!(parsed.head, head);
    assert_eq!(parsed_root, root);

    let run_key = velnor_actions_contract::run_key_for_ci(71, 1);
    let merge_request = crate::merge_request::assemble_with_needs(
        &run_key,
        temp.path(),
        Some("{}"),
        Some("[]"),
        None,
        None,
    )
    .expect("assemble merge request");
    let report = crate::merge::merge_internal_with_context(
        &merge_request,
        &crate::merge::MergeContext::detached(),
    )
    .expect("existing merge parser accepts assembled request");
    serde_json::from_str::<velnor_actions_contract::FinalReport>(&report)
        .expect("merge response is a final report");
}

#[cfg(test)]
#[test]
fn seed_admission_rejects_wrong_repository_base_or_head_context() {
    let transition =
        crate::homebrew::adoption::ProtectedHomebrewAdoptionTransition::from_protected_source(
            include_bytes!("../../../.velnor/qualification/homebrew/jackin/transition.json"),
        )
        .expect("protected transition")
        .transition()
        .clone();
    let repository = "jackin-project/homebrew-tap";
    let head = "a".repeat(40);
    let valid_pull = || crate::package_release_publish::PublicPullRequestSnapshot {
        state: "open".to_owned(),
        draft: false,
        merged: false,
        base_repository: repository.to_owned(),
        base_repository_id: 1_200_197_694,
        base_ref: "main".to_owned(),
        base_sha: transition.base_sha.clone(),
        head_repository: repository.to_owned(),
        head_repository_id: 1_200_197_694,
        head_ref: "preview-seed".to_owned(),
        head_sha: head.clone(),
        merge_commit_sha: None,
    };
    let admitted = |pull: &crate::package_release_publish::PublicPullRequestSnapshot,
                    repository_id: u64,
                    transition_id: &str| {
        seed_pull_context_matches(
            &transition,
            transition_id,
            repository,
            repository_id,
            pull,
            &head,
        )
    };
    assert!(admitted(
        &valid_pull(),
        1_200_197_694,
        &transition.transition_id
    ));

    let mut wrong_repository = valid_pull();
    wrong_repository.base_repository = "attacker/homebrew-tap".to_owned();
    assert_seed_pull_rejected(&admitted, &wrong_repository, 1_200_197_694, &transition);
    let mut wrong_base = valid_pull();
    wrong_base.base_sha = "b".repeat(40);
    assert_seed_pull_rejected(&admitted, &wrong_base, 1_200_197_694, &transition);
    let mut wrong_head = valid_pull();
    wrong_head.head_sha = "c".repeat(40);
    assert_seed_pull_rejected(&admitted, &wrong_head, 1_200_197_694, &transition);
    assert!(!admitted(&valid_pull(), 7, &transition.transition_id));
    assert!(!admitted(
        &valid_pull(),
        1_200_197_694,
        "renamed-transition"
    ));
}

fn assert_seed_pull_rejected(
    admitted: &impl Fn(&crate::package_release_publish::PublicPullRequestSnapshot, u64, &str) -> bool,
    pull: &crate::package_release_publish::PublicPullRequestSnapshot,
    repository_id: u64,
    transition: &velnor_actions_contract::homebrew_adoption::HomebrewAdoptionTransition,
) {
    assert!(!admitted(pull, repository_id, &transition.transition_id));
}

#[cfg(test)]
#[test]
fn required_seed_checks_cannot_be_renamed_or_rebound() {
    let transition =
        crate::homebrew::adoption::ProtectedHomebrewAdoptionTransition::from_protected_source(
            include_bytes!("../../../.velnor/qualification/homebrew/jackin/transition.json"),
        )
        .expect("protected transition");
    let transition = transition.transition();
    let sdk = crate::homebrew::sdk_binding::PublishedSdkBindingIdentity {
        repository: "tailrocks/velnor-new",
        source_sha: "b".repeat(40),
        version: "0.1.53".to_owned(),
        manifest_sha256: "c".repeat(64),
        seed_runner_target: "x86_64-unknown-linux-gnu",
        seed_runner_binary_sha256: "d".repeat(64),
        host_target: "x86_64-unknown-linux-gnu",
        host_binary_sha256: "d".repeat(64),
    };
    let digest = transition.digest().expect("transition digest");
    for operation in transition.required_operations.iter().copied() {
        assert_required_operation_metadata(transition, &sdk, operation, &digest);
    }
}

fn assert_required_operation_metadata(
    transition: &velnor_actions_contract::homebrew_adoption::HomebrewAdoptionTransition,
    sdk: &crate::homebrew::sdk_binding::PublishedSdkBindingIdentity,
    operation: NativeHomebrewOperation,
    digest: &str,
) {
    let metadata = seed_metadata(transition, sdk, operation, digest, None, None);
    assert!(seed_check_metadata_matches(
        Stack::Native.id(),
        &metadata,
        operation,
        transition,
        digest,
        sdk,
    ));
    assert!(!seed_required_operation_present(
        std::iter::empty(),
        operation,
        transition,
        digest,
        sdk,
    ));
    assert_rebound_metadata_rejected(transition, sdk, operation, digest, &metadata);
}

fn assert_rebound_metadata_rejected(
    transition: &velnor_actions_contract::homebrew_adoption::HomebrewAdoptionTransition,
    sdk: &crate::homebrew::sdk_binding::PublishedSdkBindingIdentity,
    operation: NativeHomebrewOperation,
    digest: &str,
    metadata: &serde_json::Value,
) {
    let renamed = serde_json::json!({"operation": "renamed", "identity": metadata["identity"]});
    assert!(!seed_check_metadata_matches(
        Stack::Native.id(),
        &renamed,
        operation,
        transition,
        digest,
        sdk
    ));
    assert!(!seed_required_operation_present(
        [(Stack::Native.id(), &renamed)],
        operation,
        transition,
        digest,
        sdk,
    ));
    assert!(!seed_check_metadata_matches(
        Stack::Native.id(),
        &serde_json::Value::Null,
        operation,
        transition,
        digest,
        sdk,
    ));
    let wrong_sdk = seed_metadata(
        transition,
        sdk,
        operation,
        digest,
        Some("e".repeat(40)),
        None,
    );
    assert!(!seed_check_metadata_matches(
        Stack::Native.id(),
        &wrong_sdk,
        operation,
        transition,
        digest,
        sdk
    ));
    let wrong_base = seed_metadata(
        transition,
        sdk,
        operation,
        digest,
        None,
        Some("f".repeat(40)),
    );
    assert!(!seed_check_metadata_matches(
        Stack::Native.id(),
        &wrong_base,
        operation,
        transition,
        digest,
        sdk
    ));
}

fn seed_metadata(
    transition: &velnor_actions_contract::homebrew_adoption::HomebrewAdoptionTransition,
    sdk: &crate::homebrew::sdk_binding::PublishedSdkBindingIdentity,
    operation: NativeHomebrewOperation,
    digest: &str,
    source_sha: Option<String>,
    base_sha: Option<String>,
) -> serde_json::Value {
    serde_json::json!({
        "operation": operation.as_str(),
        "identity": {
            "tap_base_sha": base_sha.unwrap_or_else(|| transition.base_sha.clone()),
            "authority": {
                "kind": "seed",
                "transition_id": transition.transition_id,
                "transition_digest": digest,
                "base_tree": transition.base_tree,
                "sdk_source_sha": source_sha.unwrap_or_else(|| sdk.source_sha.clone()),
                "sdk_version": sdk.version,
                "sdk_manifest_sha256": sdk.manifest_sha256,
                "sdk_seed_runner_binary_sha256": sdk.seed_runner_binary_sha256,
            }
        }
    })
}

#[cfg(test)]
#[test]
fn caller_repository_must_match_checkout_origin() {
    assert!(caller_repository_matches(
        "Jackin-Project/homebrew-tap",
        "jackin-project/homebrew-tap",
    ));
    assert!(!caller_repository_matches(
        "jackin-project/homebrew-tap",
        "jackin-project/jackin",
    ));
    assert!(!caller_repository_matches(
        "invalid",
        "jackin-project/homebrew-tap"
    ));
}

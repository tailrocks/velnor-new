//! Exact original authorization and native-tag observation regressions.

use super::*;
use serde_json::{Value, json};
use velnor_actions_contract::{
    PackageReleasePlanProduct, package_release::PackageReleasePayloadFormat,
};

pub(super) fn fixture() -> (PackageReleasePlan, Authorization, ArtifactRef) {
    let plan = PackageReleasePlan {
        schema: velnor_actions_contract::PACKAGE_RELEASE_PLAN_SCHEMA_V1.into(),
        repository: "example/app".into(),
        default_branch: "main".into(),
        channel: PackageReleaseChannel::Stable,
        base_version: "1.2.3".into(),
        products: vec![PackageReleasePlanProduct {
            id: "app".into(),
            package: "app".into(),
            bins: vec!["app".into()],
            targets: vec!["x86_64-unknown-linux-gnu".into()],
            format: PackageReleasePayloadFormat::TarGz,
        }],
        companions: vec![],
    };
    let intent = Authorization {
        schema: "velnor.stable-tag-authorization.v1".into(),
        repository: plan.repository.clone(),
        policy_sha256: "e".repeat(64),
        plan_sha256: sha256(&plan.canonical_bytes().expect("plan")),
        version_package: "app".into(),
        tag_was_absent: true,
        context: ReleaseProductionContext {
            repository_id: 7,
            head_repository_id: 7,
            source_sha: "a".repeat(40),
            source_ref: "refs/heads/main".into(),
            workflow_authority_sha: "a".repeat(40),
            run_id: 11,
            run_attempt: 1,
            ci_run_id: 9,
            ci_run_attempt: 1,
            commit_count: 0,
            version: "1.2.3".into(),
        },
    };
    (
        plan,
        intent,
        ArtifactRef {
            id: 42,
            sha256: "c".repeat(64),
        },
    )
}

#[test]
fn original_intent_completion_is_only_a_later_attempt_of_the_same_authority() {
    let (_, original, _) = fixture();
    let mut current = original.clone();
    current.context.run_attempt = 2;
    recovery::require_completion(&original, &current).expect("same-run completion");
    for mutate in [
        |intent: &mut Authorization| intent.context.run_id += 1,
        |intent: &mut Authorization| intent.context.repository_id += 1,
        |intent: &mut Authorization| intent.context.head_repository_id += 1,
        |intent: &mut Authorization| intent.context.source_sha = "b".repeat(40),
        |intent: &mut Authorization| intent.context.source_ref = "refs/heads/other".into(),
        |intent: &mut Authorization| intent.context.workflow_authority_sha = "b".repeat(40),
        |intent: &mut Authorization| intent.context.ci_run_id += 1,
        |intent: &mut Authorization| intent.context.ci_run_attempt += 1,
        |intent: &mut Authorization| intent.context.version = "9.9.9".into(),
        |intent: &mut Authorization| intent.plan_sha256 = "d".repeat(64),
        |intent: &mut Authorization| intent.policy_sha256 = "d".repeat(64),
        |intent: &mut Authorization| intent.version_package = "other".into(),
        |intent: &mut Authorization| intent.tag_was_absent = false,
    ] {
        let mut changed = current.clone();
        mutate(&mut changed);
        assert!(recovery::require_completion(&original, &changed).is_err());
    }
    assert!(recovery::require_completion(&original, &original).is_err());
    assert!(recovery::require_completion(&current, &original).is_err());
}

fn route(path: &str) -> String {
    format!("repos/example/app/{path}")
}
fn reference() -> Value {
    json!({"ref":"refs/tags/v1.2.3", "object":{"type":"tag","sha":"b".repeat(40)}})
}
fn object(message: &str) -> Value {
    json!({"sha":"b".repeat(40),"tag":"v1.2.3","message":message,"object":{"type":"commit","sha":"a".repeat(40)}})
}

fn retained(authorization: &Authorization) -> Intent {
    Intent {
        schema: "velnor.stable-tag-intent.v1".into(),
        authorization: authorization.clone(),
        tag: Tag {
            name: "v1.2.3".into(),
            object_sha: "b".repeat(40),
            object_type: "tag".into(),
            target_sha: authorization.context.source_sha.clone(),
            message: tag::authorization_message(authorization).expect("message"),
        },
    }
}

#[test]
fn existing_same_target_copied_annotation_at_another_object_is_not_owned() {
    let (plan, authorization, _) = fixture();
    let retained = retained(&authorization);
    let mut other_ref = reference();
    other_ref["object"]["sha"] = json!("c".repeat(40));
    let mut other_object = object(&retained.tag.message);
    other_object["sha"] = json!("c".repeat(40));
    let (api, calls) = GitHubApi::with_fixture(
        "example/app",
        vec![
            (
                route(&format!("git/tags/{}", "b".repeat(40))),
                object(&retained.tag.message),
            ),
            (
                route("git/matching-refs/tags/v1.2.3"),
                json!([other_ref.clone()]),
            ),
            (
                route(&format!("git/tags/{}", "c".repeat(40))),
                other_object.clone(),
            ),
            (route("git/ref/tags/v1.2.3"), other_ref.clone()),
            (route(&format!("git/tags/{}", "c".repeat(40))), other_object),
            (route("git/ref/tags/v1.2.3"), other_ref),
        ],
    );
    let error =
        tag::create_or_resume_tag(&api, &plan, &retained).expect_err("copied tag is not owned");
    assert!(error.to_string().contains("stable_tag_preexisting_unowned"));
    calls.assert_drained();
}

#[test]
fn resumed_tag_is_exactly_the_object_retained_before_permanent_ref_creation() {
    let (plan, authorization, _) = fixture();
    let retained = retained(&authorization);
    let responses = vec![
        (
            route(&format!("git/tags/{}", "b".repeat(40))),
            object(&retained.tag.message),
        ),
        (route("git/matching-refs/tags/v1.2.3"), json!([reference()])),
        (
            route(&format!("git/tags/{}", "b".repeat(40))),
            object(&retained.tag.message),
        ),
        (route("git/ref/tags/v1.2.3"), reference()),
        (
            route(&format!("git/tags/{}", "b".repeat(40))),
            object(&retained.tag.message),
        ),
        (route("git/ref/tags/v1.2.3"), reference()),
    ];
    let (api, calls) = GitHubApi::with_fixture("example/app", responses);
    let observed = tag::create_or_resume_tag(&api, &plan, &retained).expect("exact owned object");
    assert_eq!(observed, retained.tag);
    calls.assert_drained();
}

#[test]
fn tag_observation_rejects_ref_rebinding_and_wrong_embedded_object() {
    let (plan, _, _) = fixture();
    for rebound in [true, false] {
        let mut tag_object = object("intent");
        let mut final_ref = reference();
        if rebound {
            final_ref["object"]["sha"] = json!("c".repeat(40));
        } else {
            tag_object["sha"] = json!("c".repeat(40));
        }
        let mut responses = vec![
            (route("git/ref/tags/v1.2.3"), reference()),
            (route(&format!("git/tags/{}", "b".repeat(40))), tag_object),
        ];
        if rebound {
            responses.push((route("git/ref/tags/v1.2.3"), final_ref));
        }
        let (api, calls) = GitHubApi::with_fixture("example/app", responses);
        assert!(tag::observe_tag(&api, &plan).is_err());
        calls.assert_drained();
    }
}

#[test]
fn completion_origin_cannot_substitute_an_old_attempt_or_different_retained_object() {
    let (_, original, reference) = fixture();
    let mut completed = original.clone();
    completed.context.run_attempt = 2;
    let mut origin = Origin {
        schema: "velnor.stable-tag-origin.v1".into(),
        intent: reference.clone(),
        authorization: original.clone(),
        completion_context: completed.context.clone(),
        tag: Tag {
            name: "v1.2.3".into(),
            object_sha: "b".repeat(40),
            object_type: "tag".into(),
            target_sha: original.context.source_sha.clone(),
            message: tag::authorization_message(&original).expect("annotation"),
        },
    };
    artifacts::validate_origin(&origin, &completed).expect("explicit completion");
    assert!(artifacts::validate_origin(&origin, &original).is_err());
    let mut retained = retained(&original);
    artifacts::require_intent_object(&origin, &retained).expect("exact retained object");
    retained.tag.object_sha = "d".repeat(40);
    assert!(artifacts::require_intent_object(&origin, &retained).is_err());
    origin.completion_context.run_id += 1;
    assert!(artifacts::validate_origin(&origin, &completed).is_err());
}

#[test]
fn native_metadata_decoder_rejects_duplicate_keys_extra_fields_and_noncanonical_bytes() {
    let (_, authorization, _) = fixture();
    let original = retained(&authorization);
    let bytes = canonical(&original).expect("canonical intent");
    assert_eq!(
        artifacts::decode::<Intent>(&bytes).expect("strict intent"),
        original
    );
    let mut value = serde_json::to_value(&original).expect("intent");
    value["release_plz_version"] = json!("0.3.173");
    assert!(artifacts::decode::<Intent>(&canonical(&value).expect("JSON")).is_err());
    assert!(artifacts::decode::<Intent>(br#"{"schema":"one","schema":"two"}"#).is_err());
    let mut spaced = bytes;
    spaced.push(b'\n');
    assert!(artifacts::decode::<Intent>(&spaced).is_err());
}

#[test]
fn detached_object_response_must_match_exact_authorization_before_retention() {
    let (_, authorization, _) = fixture();
    let message = tag::authorization_message(&authorization).expect("annotation");
    let valid = object(&message);
    let decode = |value| serde_json::from_value(value).expect("tag response");
    tag::validate_created_object(
        &decode(valid.clone()),
        "v1.2.3",
        &message,
        &authorization.context.source_sha,
    )
    .expect("exact detached object");
    for mutate in [
        |value: &mut Value| value["sha"] = json!("not-a-sha"),
        |value: &mut Value| value["tag"] = json!("v9.9.9"),
        |value: &mut Value| value["message"] = json!("foreign"),
        |value: &mut Value| value["object"]["type"] = json!("tag"),
        |value: &mut Value| value["object"]["sha"] = json!("d".repeat(40)),
    ] {
        let mut changed = valid.clone();
        mutate(&mut changed);
        assert!(
            tag::validate_created_object(
                &decode(changed),
                "v1.2.3",
                &message,
                &authorization.context.source_sha
            )
            .is_err()
        );
    }
}

fn detached_post(message: &str) -> Value {
    json!({"expected_fields": {
        "tag":"v1.2.3", "message":message, "object":"a".repeat(40), "type":"commit",
        "tagger[name]":"Velnor Release Publisher",
        "tagger[email]":"41898282+github-actions[bot]@users.noreply.github.com"
    }, "response": object(message)})
}

#[test]
fn native_tag_protocol_detaches_before_retention_and_only_creates_the_exact_retained_ref() {
    let (plan, authorization, _) = fixture();
    let message = tag::authorization_message(&authorization).expect("message");
    let responses = vec![
        (
            format!("POST {}", route("git/tags")),
            detached_post(&message),
        ),
        (
            route(&format!("git/tags/{}", "b".repeat(40))),
            object(&message),
        ),
        (route("git/matching-refs/tags/v1.2.3"), json!([])),
        (
            format!("POST {}", route("git/refs")),
            json!({"expected_fields":{
            "ref":"refs/tags/v1.2.3", "sha":"b".repeat(40)}, "response":reference()}),
        ),
        (route("git/ref/tags/v1.2.3"), reference()),
        (
            route(&format!("git/tags/{}", "b".repeat(40))),
            object(&message),
        ),
        (route("git/ref/tags/v1.2.3"), reference()),
    ];
    let (api, calls) = GitHubApi::with_fixture("example/app", responses);
    let detached = tag::prepare_tag_object(&api, &plan, &authorization).expect("detached object");
    assert_eq!(
        calls.observed(),
        vec![format!("POST {}", route("git/tags"))]
    );
    let retained = Intent {
        schema: "velnor.stable-tag-intent.v1".into(),
        authorization,
        tag: detached,
    };
    // The production caller obtains this value only from the authenticated intent ZIP.
    let observed =
        tag::create_or_resume_tag(&api, &plan, &retained).expect("create-only exact ref");
    assert_eq!(observed, retained.tag);
    calls.assert_drained();
}

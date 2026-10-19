use super::*;
fn policy() -> ReviewCompletionPolicy {
    ReviewCompletionPolicy {
        schema: REVIEW_COMPLETION_POLICY_SCHEMA.to_owned(),
        repository: "tailrocks/homebrew-tap".to_owned(),
        default_branch: "main".to_owned(),
        provider: ReviewCompletionProvider::CodexV1,
        check_name: "Review completion".to_owned(),
    }
}
#[test]
fn completion_policy_is_canonical_closed_and_bounded() {
    let input = policy();
    let bytes = input.canonical_bytes().expect("encode");
    assert_eq!(
        parse_review_completion_policy(&bytes).expect("parse"),
        input
    );
    let mut padded = bytes.clone();
    padded.push(b'\n');
    assert!(parse_review_completion_policy(&padded).is_err());
    for extension in [
        r#", "roles": []}"#,
        r#", "schema": "velnor.review-completion-policy.v1"}"#,
    ] {
        let mut text = String::from_utf8(bytes.clone()).expect("UTF8");
        text.pop();
        text.push_str(extension);
        assert!(parse_review_completion_policy(text.as_bytes()).is_err());
    }
    assert!(
        parse_review_completion_policy(&vec![b' '; MAX_REVIEW_COMPLETION_POLICY_BYTES + 1])
            .is_err()
    );
    let mut bad = input;
    bad.repository = "other/repo/extra".to_owned();
    assert!(bad.validate().is_err());
}
#[test]
fn completion_config_has_no_arbitrary_provider_role_environment_or_task_knobs() {
    let value = serde_json::json!({"provider":"codex-v1","check_name":"Review completion","app_id_variable":"COMPLETION_APP_ID","app_private_key_secret":"COMPLETION_APP_PRIVATE_KEY","reviewer_app_id_variable":"REVIEWER_APP_ID"});
    let valid: crate::config::ReviewCompletionConfig =
        serde_json::from_value(value.clone()).expect("config");
    valid.validate("config").expect("valid");
    for field in ["environment", "roles", "tasks", "enabled", "cron"] {
        let mut extra = value.clone();
        extra[field] = serde_json::json!(true);
        assert!(serde_json::from_value::<crate::config::ReviewCompletionConfig>(extra).is_err());
    }
    let mut bad = value.clone();
    bad["provider"] = serde_json::json!("arbitrary");
    assert!(serde_json::from_value::<crate::config::ReviewCompletionConfig>(bad).is_err());
    for (field, replacement) in [
        ("check_name", "${{ bad }}"),
        ("app_id_variable", "bad-var"),
        ("app_private_key_secret", "bad-secret"),
        ("reviewer_app_id_variable", "COMPLETION_APP_ID"),
    ] {
        let mut bad = value.clone();
        bad[field] = serde_json::json!(replacement);
        let parsed: crate::config::ReviewCompletionConfig =
            serde_json::from_value(bad).expect("shape");
        assert!(parsed.validate("config").is_err());
    }
}

#[test]
fn completion_filename_and_slug_are_reserved_native_owners() {
    assert!(crate::config::GENERATOR_OWNED_WORKFLOW_FILES.contains(&"review-completion.yml"));
    assert!(crate::config::RESERVED_SUITE_WORKFLOW_SLUGS.contains(&"review-completion"));
}

use super::*;
use velnor_actions_contract::{StepKind, config::ReviewCompletionProvider};

fn spec() -> ReviewCompletionSpec {
    ReviewCompletionSpec {
        policy: ReviewCompletionPolicy {
            schema: velnor_actions_contract::review_completion::REVIEW_COMPLETION_POLICY_SCHEMA
                .to_owned(),
            repository: "tailrocks/homebrew-tap".to_owned(),
            default_branch: "main".to_owned(),
            provider: ReviewCompletionProvider::CodexV1,
            check_name: "Review completion".to_owned(),
        },
        config: ReviewCompletionConfig {
            provider: ReviewCompletionProvider::CodexV1,
            check_name: "Review completion".to_owned(),
            app_id_variable: "COMPLETION_APP_ID".to_owned(),
            app_private_key_secret: "COMPLETION_APP_PRIVATE_KEY".to_owned(),
            reviewer_app_id_variable: "REVIEWER_APP_ID".to_owned(),
        },
        generator_version: "0.1.21".to_owned(),
        setup: MiseSetup {
            uses: "jdx/mise-action@82a612c5ab8bcda7cc5db6b7e81a8e7584c3dc6e".to_owned(),
            version: "2026.10.19".to_owned(),
            sha256: crate::setup::MISE_BINARY_SHA256_LINUX_X64.to_owned(),
        },
        acquire: Step {
            parallel: false,
            name: "Acquire verified helper".to_owned(),
            id: None,
            role: None,
            condition: None,
            kind: StepKind::Shell {
                run: vec!["printf".to_owned(), "verified-fixture".to_owned()],
                env: std::collections::BTreeMap::new(),
            },
        },
        install_gh_argv: vec![
            "mise".to_owned(),
            "install".to_owned(),
            "github:cli/cli@2.103.2".to_owned(),
        ],
    }
}
fn field<'a>(node: &'a Yaml, key: &str) -> &'a Yaml {
    let Yaml::Map(rows) = node else {
        panic!("map required")
    };
    &rows.iter().find(|(name, _)| name == key).expect("field").1
}
#[test]
fn completion_workflow_has_only_closed_trusted_triggers_and_five_minute_job() {
    let file = render_review_completion(&spec()).expect("render");
    assert_eq!(file.path, REVIEW_COMPLETION_WORKFLOW_PATH);
    for required in [
        "review-completion:",
        "timeout-minutes: 5",
        "environment: review-completion",
        "pull_request_target:",
        "issue_comment:",
        "ready_for_review",
        "synchronize",
    ] {
        assert!(file.bytes.contains(required), "{required}: {}", file.bytes);
    }
    for forbidden in [
        "actions/checkout@",
        "pull_request_review:",
        "workflow_dispatch:",
        "schedule:",
        "cargo ",
        "GITHUB_ENV",
        "permission-actions",
    ] {
        assert!(!file.bytes.contains(forbidden), "{forbidden}");
    }
}
#[test]
fn completion_credentials_are_step_local_and_all_eligible_states_emit() {
    let steps = steps::render(&spec()).expect("steps");
    assert_eq!(steps.len(), 6);
    assert_eq!(field(&steps[3], "id"), &Yaml::str("capture"));
    assert_eq!(
        field(field(&steps[3], "env"), "GH_TOKEN"),
        &Yaml::str("${{ github.token }}")
    );
    assert_eq!(
        field(field(&steps[5], "env"), "GH_TOKEN"),
        &Yaml::str("${{ steps.completion-token.outputs.token }}")
    );
    for step in [&steps[4], &steps[5]] {
        assert_eq!(
            field(step, "if"),
            &Yaml::str("${{ steps.capture.outputs.eligible == 'true' }}")
        );
    }
    assert_eq!(
        field(field(&steps[4], "with"), "permission-checks"),
        &Yaml::str("write")
    );
    assert_eq!(
        field(field(&steps[4], "with"), "repositories"),
        &Yaml::str("homebrew-tap")
    );
    for (index, step) in steps.iter().enumerate() {
        let text = format!("{step:?}");
        if index != 4 {
            assert!(!text.contains("secrets."));
        }
        if index != 5 {
            assert!(!text.contains("steps.completion-token.outputs.token"));
        }
    }
    assert_eq!(
        field(
            field(&steps[5], "env"),
            "VELNOR_REVIEW_COMPLETION_PROPOSAL_DIGEST"
        ),
        &Yaml::str("${{ steps.capture.outputs.proposal_digest }}")
    );
}
#[test]
fn completion_trigger_admission_rejects_siblings_and_duplicate_on() {
    let on = ("on".to_owned(), triggers("main"));
    assert!(validate_triggers(&Yaml::Map(vec![on.clone()]), "main").is_ok());
    assert!(validate_triggers(&Yaml::Map(vec![on.clone(), on.clone()]), "main").is_err());
    let Yaml::Map(mut events) = triggers("main") else {
        panic!("events")
    };
    events.push(("push".to_owned(), Yaml::Map(Vec::new())));
    assert!(
        validate_triggers(
            &Yaml::Map(vec![("on".to_owned(), Yaml::Map(events))]),
            "main"
        )
        .is_err()
    );
    assert!(validate_triggers(&Yaml::Map(vec![on]), "other").is_err());
}
#[test]
fn completion_rejects_mismatched_policy_and_shared_app_variable() {
    let mut input = spec();
    input.policy.check_name = "Other".to_owned();
    assert!(render_review_completion(&input).is_err());
    let mut input = spec();
    input
        .config
        .reviewer_app_id_variable
        .clone_from(&input.config.app_id_variable);
    assert!(render_review_completion(&input).is_err());
}

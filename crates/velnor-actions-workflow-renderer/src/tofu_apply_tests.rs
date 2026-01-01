use super::{
    AWS_CREDENTIALS_STEP_ID, PLAN_REVIEW_JQ, TOFU_APPLY_CONCURRENCY_GROUP, TofuApplySpec,
    render_tofu_apply_workflow,
};
use crate::MiseSetup;
use velnor_actions_contract::{
    GitHubTokenSecret, S3BackendConfig, TofuApplyConfig, Utf8RepoRelDir,
};

const CHECKOUT: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";
const MISE: &str = "jdx/mise-action@2d8d4cafcbd33be2ea37d2b6f5ad595363d1f1ca";

fn spec() -> TofuApplySpec {
    TofuApplySpec {
        config: TofuApplyConfig {
            root: Utf8RepoRelDir::from_raw("infra".to_owned()),
            environment: "production".to_owned(),
            role_arn: "arn:aws:iam::123456789012:role/velnor-tofu".to_owned(),
            backend: S3BackendConfig {
                bucket: "example-tofu-state".to_owned(),
                key: "chainargos/control-plane.tfstate".to_owned(),
                region: "us-east-1".to_owned(),
            },
            github_tokens: vec![
                GitHubTokenSecret {
                    organization: "chainargos".to_owned(),
                    secret_name: "GH_TOKEN_CHAINARGOS".to_owned(),
                },
                GitHubTokenSecret {
                    organization: "tailrocks".to_owned(),
                    secret_name: "GH_TOKEN_TAILROCKS".to_owned(),
                },
            ],
        },
        default_branch: "main".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        checkout_uses: CHECKOUT.to_owned(),
        mise_setup: MiseSetup {
            uses: MISE.to_owned(),
            version: "2026.10.6".to_owned(),
            sha256: crate::setup::MISE_BINARY_SHA256_LINUX_X64.to_owned(),
        },
        opentofu_version: "1.13.1".to_owned(),
        generator_version: "0.1.0".to_owned(),
    }
}

#[test]
fn renders_only_protected_main_push_and_scoped_apply_capabilities() {
    let file = render_tofu_apply_workflow(&spec()).expect("render");
    assert_eq!(file.path, ".github/workflows/tofu-apply.yml");
    let yaml = file.bytes;
    assert!(yaml.contains("branches:\n      - main"), "{yaml}");
    assert!(!yaml.contains("pull_request"), "{yaml}");
    assert!(!yaml.contains("workflow_dispatch"), "{yaml}");
    assert!(
        yaml.contains("if: ${{ github.ref_protected == true }}"),
        "{yaml}"
    );
    assert!(yaml.contains("environment: production"), "{yaml}");
    assert!(yaml.contains("cancel-in-progress: false"), "{yaml}");
    assert!(yaml.contains("contents: none"), "{yaml}");
    assert!(yaml.contains("id-token: write"), "{yaml}");
    assert!(yaml.contains("id: aws-credentials"), "{yaml}");
    assert!(yaml.contains(AWS_CREDENTIALS_STEP_ID), "{yaml}");
    assert!(yaml.contains("use_lockfile=true"), "{yaml}");
    assert!(yaml.contains("-refresh=true"), "{yaml}");
    assert!(yaml.contains("tofu show -json"), "{yaml}");
    assert!(yaml.contains("Apply reviewed saved plan"), "{yaml}");
    assert!(yaml.contains("-refresh-only"), "{yaml}");
    assert!(yaml.contains("secrets.GH_TOKEN_CHAINARGOS"), "{yaml}");
    assert!(yaml.contains("secrets.GH_TOKEN_TAILROCKS"), "{yaml}");
    assert!(!yaml.contains("${{ secrets.AWS"), "{yaml}");
    assert!(yaml.contains(TOFU_APPLY_CONCURRENCY_GROUP), "{yaml}");
}

#[test]
fn output_matches_generator_golden_snapshot() {
    let file = render_tofu_apply_workflow(&spec()).expect("render");
    assert_eq!(
        file.bytes,
        include_str!("../tests/goldens/tofu-apply.yml"),
        "byte-exact generated workflow golden"
    );
}

#[test]
fn plan_review_rejects_destructive_or_incomplete_plans() {
    for expected in [
        ".errored == false",
        ".complete == true",
        ".applyable == true",
        ".deferred_changes == []",
        ".action_invocations == []",
        "[\"create\"]",
        "[\"update\"]",
        "[\"no-op\"]",
        "[\"read\"]",
    ] {
        assert!(PLAN_REVIEW_JQ.contains(expected), "missing {expected}");
    }
    assert!(!PLAN_REVIEW_JQ.contains("[\"delete\"]"));
}

#[test]
fn invalid_branch_and_floating_tofu_versions_fail_closed() {
    let mut config = spec();
    config.default_branch = "main && echo unsafe".to_owned();
    assert!(config.validate().is_err());
    let mut config = spec();
    config.opentofu_version = "latest".to_owned();
    assert!(config.validate().is_err());
}

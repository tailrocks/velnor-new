//! Execute the real local handoff in a child with no credentials or tool PATH.

use super::*;
use std::{fs, process::Command, time::Duration};
use velnor_actions_contract::{
    CargoVersionBinding, CargoVersionPolicy, PACKAGE_RELEASE_POLICY_SCHEMA_V1,
};

const TEST: &str = "package_release_publish::stable_tag_origin::admission::tests::stable_local_handoff_needs_no_http_and_rejects_replaced_plan";

#[test]
fn stable_local_handoff_needs_no_http_and_rejects_replaced_plan() {
    if std::env::var_os("VELNOR_TEST_STABLE_LOCAL_CHILD").is_some() {
        check_child();
        return;
    }
    let (plan, mut authorization, intent) = super::super::tests::fixture();
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
    let policy_bytes = policy.canonical_bytes().expect("policy");
    authorization.policy_sha256 = sha256(&policy_bytes);
    let origin = ArtifactRef {
        id: 43,
        sha256: "d".repeat(64),
    };
    let admission = Admission {
        schema: "velnor.stable-production-admission.v1".into(),
        intent: authorization,
        origin_archive: origin.clone(),
        intent_archive: intent.clone(),
    };
    let bytes = canonical(&admission).expect("admission");
    let root = tempfile::tempdir().expect("fixture");
    let temp = root.path().join("velnor");
    fs::create_dir_all(temp.join("stable-tag")).expect("directory");
    fs::write(temp.join("stable-tag/admission.json"), &bytes).expect("admission");
    fs::write(
        temp.join("package-release-plan.json"),
        plan.canonical_bytes().expect("plan"),
    )
    .expect("plan");
    let mut command = child(
        root.path(),
        &plan,
        std::str::from_utf8(&policy_bytes).expect("UTF8"),
    );
    command
        .env("VELNOR_STABLE_TAG_ORIGIN_ADMISSION_DIGEST", sha256(&bytes))
        .env(
            "VELNOR_STABLE_TAG_ORIGIN_ARTIFACT_ID",
            origin.id.to_string(),
        )
        .env("VELNOR_STABLE_TAG_ORIGIN_ARTIFACT_DIGEST", origin.sha256)
        .env(
            "VELNOR_STABLE_TAG_INTENT_ARTIFACT_ID",
            intent.id.to_string(),
        )
        .env("VELNOR_STABLE_TAG_INTENT_ARTIFACT_DIGEST", intent.sha256);
    assert!(command.status().expect("local handoff child").success());
    assert_eq!(
        fs::read(root.path().join("child-result")).expect("child executed"),
        b"admitted"
    );
    reject_replacement(command, plan, &temp, root.path());
}

fn reject_replacement(mut command: Command, plan: PackageReleasePlan, temp: &Path, root: &Path) {
    let mut replaced = plan;
    replaced.base_version = "1.2.4".into();
    fs::write(
        temp.join("package-release-plan.json"),
        replaced.canonical_bytes().expect("replacement"),
    )
    .expect("replace");
    assert!(
        command
            .env("VELNOR_TEST_EXPECT_REJECT", "1")
            .status()
            .expect("negative child")
            .success()
    );
    assert_eq!(
        fs::read(root.join("child-result")).expect("negative child executed"),
        b"rejected"
    );
}

fn check_child() {
    assert!(std::env::var_os("GH_TOKEN").is_none());
    let temp = std::path::PathBuf::from(required("RUNNER_TEMP").expect("temp")).join("velnor");
    let deadline = CheckDeadline::after(Duration::from_secs(2)).expect("deadline");
    let result = admitted_policy_plan(&temp, deadline);
    if std::env::var_os("VELNOR_TEST_EXPECT_REJECT").is_some() {
        assert!(result.is_err());
        fs::write(
            temp.parent().expect("temp parent").join("child-result"),
            b"rejected",
        )
        .expect("child receipt");
    } else {
        let (_, plan) = result.expect("credential-free local policy/plan");
        let context = admitted_production_context(&plan, deadline).expect("local context");
        assert_eq!(context.source_sha, "a".repeat(40));
        fs::write(
            temp.parent().expect("temp parent").join("child-result"),
            b"admitted",
        )
        .expect("child receipt");
    }
}

fn child(root: &Path, plan: &PackageReleasePlan, policy: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().expect("test binary"));
    command
        .args(["--exact", TEST])
        .env_clear()
        .env("HOME", std::env::var_os("HOME").expect("actual HOME"))
        .env("VELNOR_TEST_STABLE_LOCAL_CHILD", "1")
        .env("RUNNER_TEMP", root)
        .env("VELNOR_RELEASE_POLICY", policy)
        .env("GITHUB_REPOSITORY", &plan.repository)
        .env("GITHUB_REPOSITORY_ID", "7")
        .env("GITHUB_SHA", "a".repeat(40))
        .env("GITHUB_WORKFLOW_SHA", "a".repeat(40))
        .env("GITHUB_REF", "refs/heads/main")
        .env(
            "GITHUB_WORKFLOW_REF",
            format!(
                "{}/{}@refs/heads/main",
                plan.repository,
                plan.channel.workflow_path()
            ),
        )
        .env("GITHUB_EVENT_NAME", "workflow_dispatch")
        .env("GITHUB_RUN_ID", "11")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env("VELNOR_RELEASE_CI_RUN_ID", "9")
        .env("VELNOR_RELEASE_CI_RUN_ATTEMPT", "1");
    command
}

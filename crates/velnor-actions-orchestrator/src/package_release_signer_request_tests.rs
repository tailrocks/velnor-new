use super::*;
use serde_json::{Value, json};
use std::{fs, process::Command};
use velnor_actions_contract::{
    CargoVersionBinding, CargoVersionPolicy, PACKAGE_RELEASE_POLICY_SCHEMA_V1,
    canonical_json_bytes, config::RustBinaryReleaseProducer,
    preview_production::ReleaseProductionContext,
};

const CHILD: &str = "package_release_signer::request::tests::static_policy_request_loads_only_digest_pinned_channel_admission";

pub(super) fn policy(channel: PackageReleaseChannel) -> PackageReleasePolicy {
    PackageReleasePolicy {
        schema: PACKAGE_RELEASE_POLICY_SCHEMA_V1.into(),
        channel,
        repository: "jackin-project/jackin".into(),
        default_branch: "main".into(),
        products: vec![velnor_actions_contract::PackageReleasePlanProduct {
            id: "jackin".into(),
            package: "jackin".into(),
            bins: vec!["jackin".into()],
            targets: vec!["x86_64-unknown-linux-gnu".into()],
            format: velnor_actions_contract::package_release::PackageReleasePayloadFormat::TarGz,
        }],
        companions: vec![],
        version: CargoVersionPolicy {
            version_package: "jackin".into(),
            bindings: vec![CargoVersionBinding {
                package: "jackin".into(),
                manifest_path: "Cargo.toml".into(),
                workspace_manifest_path: "Cargo.toml".into(),
            }],
        },
    }
}

#[test]
fn static_policy_request_loads_only_digest_pinned_channel_admission() {
    if env::var_os("VELNOR_SIGNER_REQUEST_CHILD").is_some() {
        let root = absolute_env("RUNNER_TEMP").expect("runner").join("velnor");
        if env::var_os("VELNOR_DIRECT_NEGATIVE").is_some() {
            let error = direct_tap_plan(&root).expect_err("wrong direct event/operation");
            assert!(format!("{error:?}").contains("package_tap_event_invalid"));
            return;
        }
        let result = admitted_plan(&root);
        if env::var_os("VELNOR_EXPECT_REJECT").is_some() {
            assert!(result.is_err(), "accepted substituted request/admission");
        } else {
            assert_eq!(
                result.expect("admitted static policy request").base_version,
                "1.2.3"
            );
        }
        if env::var_os("VELNOR_EXPECT_REJECT").is_none() {
            let operation = env::var("VELNOR_INTERNAL_OP").expect("actual operation");
            let result = if operation == "sign-package-release-v1" {
                super::super::sign_package_release_from_env()
            } else {
                super::super::publisher_context_from_env(None).map(|_| ())
            };
            let error = result.expect_err("fixture deliberately has no producer directory");
            assert!(
                format!("{error:?}").contains("package_signer_directory_missing"),
                "actual entry must pass static policy admission before staged bytes: {error:?}"
            );
        }
        return;
    }
    exercise_channel_requests();
}

fn exercise_channel_requests() {
    for channel in [
        PackageReleaseChannel::Preview,
        PackageReleaseChannel::Stable,
    ] {
        for mutation in [
            "none",
            "legacy-path",
            "policy",
            "plan",
            "digest",
            "source",
            "attempt",
            "missing",
        ] {
            let temp = tempfile::tempdir().expect("private fixture");
            let runner = temp.path().canonicalize().expect("canonical runner");
            let root = runner.join("velnor");
            fs::create_dir(&root).expect("root");
            let mut command = fixture(&runner, &root, channel);
            for operation in ["sign-package-release-v1", "publish-package-release-v1"] {
                let generated = super::rendered_tests::request_path(channel, operation);
                let path =
                    generated.replace("${{ runner.temp }}", runner.to_str().expect("runner UTF8"));
                command
                    .env("VELNOR_REQUEST_FILE", path)
                    .env("VELNOR_INTERNAL_OP", operation);
                let output = command.output().expect("generated operation handoff");
                assert!(
                    output.status.success(),
                    "{channel:?}/{operation}: {}",
                    String::from_utf8_lossy(&output.stdout)
                );
            }
            mutate(&mut command, &root, channel, mutation);
            if mutation != "none" {
                command.env("VELNOR_EXPECT_REJECT", "1");
            }
            let output = command.output().expect("isolated actual local loader");
            assert!(
                output.status.success(),
                "{channel:?}/{mutation}: {}",
                String::from_utf8_lossy(&output.stdout)
            );
        }
    }
}

fn mutate(command: &mut Command, root: &Path, channel: PackageReleaseChannel, mutation: &str) {
    match mutation {
        "legacy-path" => {
            command.env(
                "VELNOR_REQUEST_FILE",
                root.join("package-release-plan.json"),
            );
        }
        "policy" => {
            fs::write(root.join(POLICY_FILE), b"{}").expect("replace policy");
        }
        "plan" => {
            let mut plan = policy(channel).resolve("1.2.3".into()).expect("plan");
            plan.base_version = "9.9.9".into();
            fs::write(
                root.join("package-release-plan.json"),
                plan.canonical_bytes().expect("plan"),
            )
            .expect("replace plan");
        }
        "digest" => {
            command.env(
                if channel == PackageReleaseChannel::Preview {
                    "VELNOR_RELEASE_POLICY_ADMISSION_DIGEST"
                } else {
                    "VELNOR_STABLE_TAG_ORIGIN_ADMISSION_DIGEST"
                },
                "0".repeat(64),
            );
        }
        "source" => {
            command.env("GITHUB_SHA", "b".repeat(40));
        }
        "attempt" => {
            command.env("GITHUB_RUN_ATTEMPT", "2");
        }
        "missing" => {
            fs::remove_file(root.join(if channel == PackageReleaseChannel::Preview {
                "release-policy-admission.json"
            } else {
                "stable-tag/admission.json"
            }))
            .expect("remove admission");
        }
        _ => {}
    }
}

fn fixture(runner: &Path, root: &Path, channel: PackageReleaseChannel) -> Command {
    let policy = policy(channel);
    let plan = policy.resolve("1.2.3".into()).expect("plan");
    let policy_bytes = policy.canonical_bytes().expect("policy");
    let plan_bytes = plan.canonical_bytes().expect("plan");
    let source = "a".repeat(40);
    let context = ReleaseProductionContext {
        repository_id: 42,
        head_repository_id: 42,
        source_sha: source.clone(),
        source_ref: "refs/heads/main".into(),
        workflow_authority_sha: source.clone(),
        run_id: 10,
        run_attempt: 1,
        ci_run_id: 9,
        ci_run_attempt: 1,
        commit_count: if channel == PackageReleaseChannel::Preview {
            7
        } else {
            0
        },
        version: if channel == PackageReleaseChannel::Preview {
            format!("1.2.3-preview.7+{}", &source[..7])
        } else {
            "1.2.3".into()
        },
    };
    fs::write(root.join(POLICY_FILE), &policy_bytes).expect("policy file");
    fs::write(root.join("package-release-plan.json"), &plan_bytes).expect("plan file");
    let admission = admission(channel, &context, &policy_bytes, &plan_bytes);
    let bytes = canonical_json_bytes(&admission).expect("canonical admission");
    let path = if channel == PackageReleaseChannel::Preview {
        root.join("release-policy-admission.json")
    } else {
        fs::create_dir(root.join("stable-tag")).expect("tag root");
        root.join("stable-tag/admission.json")
    };
    fs::write(path, &bytes).expect("local admission");
    fs::write(root.join("event.json"), serde_json::to_vec(&json!({
        "action":"completed", "repository":{"id":42,"full_name":plan.repository},
        "workflow_run":{"id":9,"run_attempt":1,"head_repository":{"id":42},"head_sha":source,
            "head_branch":"main","event":"push","status":"completed","conclusion":"success",
            "path":velnor_actions_contract::CI_WORKFLOW_PATH,"name":velnor_actions_contract::WORKFLOW_DISPLAY_NAME}
    })).expect("event")).expect("event file");
    let mut command = child(runner, root, &plan, &policy_bytes, &source);
    if channel == PackageReleaseChannel::Preview {
        command.env(
            "VELNOR_RELEASE_POLICY_ADMISSION_DIGEST",
            super::super::sha256(&bytes),
        );
    } else {
        command
            .env(
                "VELNOR_STABLE_TAG_ORIGIN_ADMISSION_DIGEST",
                super::super::sha256(&bytes),
            )
            .env("VELNOR_STABLE_TAG_ORIGIN_ARTIFACT_ID", "43")
            .env("VELNOR_STABLE_TAG_ORIGIN_ARTIFACT_DIGEST", "d".repeat(64))
            .env("VELNOR_STABLE_TAG_INTENT_ARTIFACT_ID", "44")
            .env("VELNOR_STABLE_TAG_INTENT_ARTIFACT_DIGEST", "e".repeat(64));
    }
    command
}

fn child(
    runner: &Path,
    root: &Path,
    plan: &PackageReleasePlan,
    policy_bytes: &[u8],
    source: &str,
) -> Command {
    let mut command = Command::new(env::current_exe().expect("test executable"));
    command
        .args(["--exact", CHILD])
        .env_clear()
        .env("HOME", env::var_os("HOME").expect("HOME"))
        .env("VELNOR_SIGNER_REQUEST_CHILD", "1")
        .env("RUNNER_TEMP", runner)
        .env("VELNOR_REQUEST_FILE", root.join(POLICY_FILE))
        .env(
            "VELNOR_RELEASE_POLICY",
            String::from_utf8(policy_bytes.to_vec()).expect("policy UTF8"),
        )
        .env("GITHUB_REPOSITORY", &plan.repository)
        .env("GITHUB_REPOSITORY_ID", "42")
        .env("GITHUB_SHA", source)
        .env("GITHUB_WORKFLOW_SHA", source)
        .env("VELNOR_RELEASE_SOURCE_SHA", source)
        .env("VELNOR_RELEASE_WORKFLOW_AUTHORITY_SHA", source)
        .env("GITHUB_REF", "refs/heads/main")
        .env(
            "GITHUB_WORKFLOW_REF",
            format!(
                "{}/{}@refs/heads/main",
                plan.repository,
                plan.channel.workflow_path()
            ),
        )
        .env("GITHUB_EVENT_NAME", plan.channel.event().as_str())
        .env("GITHUB_EVENT_PATH", root.join("event.json"))
        .env("GITHUB_RUN_ID", "10")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env("VELNOR_RELEASE_CI_RUN_ID", "9")
        .env("VELNOR_RELEASE_CI_RUN_ATTEMPT", "1")
        .env("VELNOR_RELEASE_COMMIT_COUNT", "7");
    command
}

fn admission(
    channel: PackageReleaseChannel,
    context: &ReleaseProductionContext,
    policy: &[u8],
    plan: &[u8],
) -> Value {
    if channel == PackageReleaseChannel::Preview {
        let producer = RustBinaryReleaseProducer {
            task: "release-preview".to_owned(),
            ..RustBinaryReleaseProducer::default()
        };
        json!({"schema":"velnor.preview-source-admission.v1", "policy_sha256":super::super::sha256(policy),
            "plan_sha256":super::super::sha256(plan), "context":context, "source_files":[],
            "producer":serde_json::to_value(producer).expect("serialized producer"), "task_config":"", "task_inputs":[],
            "tool_specs":[], "qualified_tools":[], "qualification_digest":"f".repeat(64)})
    } else {
        json!({"schema":"velnor.stable-production-admission.v1", "intent":{
            "schema":"velnor.stable-tag-authorization.v1", "repository":"jackin-project/jackin",
            "policy_sha256":super::super::sha256(policy), "plan_sha256":super::super::sha256(plan),
            "version_package":"jackin", "context":context, "tag_was_absent":true},
            "origin_archive":{"id":43,"sha256":"d".repeat(64)}, "intent_archive":{"id":44,"sha256":"e".repeat(64)}})
    }
}

#[test]
fn only_explicit_tap_reads_can_use_direct_or_retained_authority() {
    for op in [
        "admit-homebrew-tap-v1",
        "write-homebrew-tap-pr-v1",
        "merge-homebrew-tap-pr-v1",
    ] {
        for event in ["workflow_run", "workflow_dispatch"] {
            assert_eq!(
                route(Some(op), Some(event), None, Some("")).expect("direct"),
                VerificationRoute::DirectTap
            );
        }
        for event in ["workflow_dispatch", "schedule"] {
            assert_eq!(
                route(Some(op), Some(event), Some("11"), Some("2")).expect("retained"),
                VerificationRoute::Retained
            );
        }
        for (event, id, attempt) in [
            ("workflow_run", Some("11"), Some("2")),
            ("schedule", None, None),
            ("push", None, None),
            ("workflow_dispatch", Some("11"), None),
        ] {
            assert!(route(Some(op), Some(event), id, attempt).is_err());
        }
    }
    for op in [
        None,
        Some("sign-package-release-v1"),
        Some("publish-package-release-v1"),
        Some("prepare-homebrew-tap-v1"),
        Some("recover-package-release-v1"),
        Some("unknown"),
    ] {
        assert_eq!(
            route(op, Some("workflow_dispatch"), Some("11"), Some("2")).expect("signing path"),
            VerificationRoute::Admitted
        );
    }
}

#[test]
fn direct_tap_entry_rejects_cross_channel_events_and_stable_nonrelease_before_http() {
    for channel in [
        PackageReleaseChannel::Preview,
        PackageReleaseChannel::Stable,
    ] {
        for operation in [
            "admit-homebrew-tap-v1",
            "write-homebrew-tap-pr-v1",
            "merge-homebrew-tap-pr-v1",
        ] {
            let temp = tempfile::tempdir().expect("private fixture");
            let runner = temp.path().canonicalize().expect("canonical runner");
            let root = runner.join("velnor");
            fs::create_dir(&root).expect("root");
            let mut command = fixture(&runner, &root, channel);
            command
                .env("VELNOR_INTERNAL_OP", operation)
                .env("VELNOR_DIRECT_NEGATIVE", "1")
                .env(
                    "GITHUB_EVENT_NAME",
                    if channel == PackageReleaseChannel::Preview {
                        "workflow_dispatch"
                    } else {
                        "workflow_run"
                    },
                );
            assert!(command.status().expect("cross-channel child").success());
            if channel == PackageReleaseChannel::Stable {
                command.env("GITHUB_EVENT_NAME", "workflow_dispatch");
                for phase in ["prepare", "", "invalid"] {
                    assert!(
                        command
                            .env("VELNOR_STABLE_OPERATION", phase)
                            .status()
                            .expect("phase child")
                            .success()
                    );
                }
            }
        }
    }
}

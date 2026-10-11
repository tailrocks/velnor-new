use std::collections::BTreeSet;

use velnor_actions_contract::{RoutingWorkflow, SCALE_SET_NAME, VELNOR_LABEL};

use super::{
    MbxQualificationPins, MisePinQualificationPins, RustToolchainQualificationPins,
    Schema2WorkflowRequest, monitoring, qualification, render_schema2_workflows,
};
use crate::setup::MiseSetup;
use crate::yaml::Yaml;

fn request() -> Schema2WorkflowRequest {
    Schema2WorkflowRequest {
        version: "2.0.0".to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set().expect("valid scale set"),
        workflows: BTreeSet::from([RoutingWorkflow::Qualification]),
        mbx_qualification: Some(MbxQualificationPins {
            mise_setup: MiseSetup {
                uses: format!("jdx/mise-action@{}", "a".repeat(40)),
                version: "2026.1.0".to_owned(),
                sha256: "a".repeat(64),
            },
            candidate_action_uses: format!(
                "{}@{}",
                crate::cache_steps::MBX_ACTION_NAME,
                "b".repeat(40)
            ),
            mbx_version: "1.0.0".to_owned(),
            rust_version: "1.98.1".to_owned(),
        }),
        mise_pin_qualification: Some(MisePinQualificationPins {
            linux_x86_64_setup: MiseSetup {
                uses: format!("jdx/mise-action@{}", "a".repeat(40)),
                version: "2026.10.5".to_owned(),
                sha256: "b".repeat(64),
            },
            macos_x86_64_setup: MiseSetup {
                uses: format!("jdx/mise-action@{}", "a".repeat(40)),
                version: "2026.10.5".to_owned(),
                sha256: "c".repeat(64),
            },
        }),
        rust_toolchain_qualification: Some(RustToolchainQualificationPins {
            mise_setup: MiseSetup {
                uses: format!("jdx/mise-action@{}", "a".repeat(40)),
                version: velnor_actions_mise::MISE_VERSION.to_owned(),
                sha256: crate::setup::MISE_BINARY_SHA256_LINUX_X64.to_owned(),
            },
            mbx_version: velnor_actions_mise::MR_BOXINGTON_VERSION.to_owned(),
            rust_version: "1.99.0".to_owned(),
            manifest_url: "https://static.rust-lang.org/dist/channel-rust-1.99.0.toml".to_owned(),
            manifest_sha256: super::super::RUST_TOOLCHAIN_QUALIFICATION_MANIFEST_SHA256.to_owned(),
        }),
        product_release: None,
    }
}

fn assert_lane_shells(document: &Yaml, minimum_per_lane: usize) {
    let Yaml::Map(document) = document else {
        panic!("workflow document is not a mapping");
    };
    let Some((_, Yaml::Map(jobs))) = document.iter().find(|(key, _)| key == "jobs") else {
        panic!("workflow jobs are missing");
    };
    let mut hosted = 0;
    let mut scale_set = 0;
    let mut containers = 0;
    for (id, job) in jobs {
        let Yaml::Map(fields) = job else {
            panic!("job {id} is not a mapping");
        };
        let Some((_, runs_on)) = fields.iter().find(|(key, _)| key == "runs-on") else {
            panic!("job {id} has no runs-on");
        };
        let defaults = fields.iter().find(|(key, _)| key == "defaults");
        let has_container = fields.iter().any(|(key, _)| key == "container");
        if has_container {
            containers += 1;
        }
        let expected_shell = if has_container {
            crate::runs_on::CONTAINER_RUN_SHELL
        } else {
            crate::runs_on::SCALE_SET_RUN_SHELL
        };
        match runs_on {
            Yaml::Flow(labels) => {
                assert_eq!(
                    labels,
                    &[VELNOR_LABEL.to_owned(), SCALE_SET_NAME.to_owned()],
                    "{id}"
                );
                scale_set += 1;
                assert_eq!(
                    defaults.map(|(_, value)| value),
                    Some(&Yaml::Map(vec![(
                        "run".to_owned(),
                        Yaml::Map(vec![("shell".to_owned(), Yaml::str(expected_shell))]),
                    )])),
                    "scale-set job {id} must declare its shell"
                );
            }
            Yaml::Str(label)
                if label == "ubuntu-26.04" || label == "macos-15-intel" || label == "macos-26" =>
            {
                hosted += 1;
                if has_container {
                    assert_eq!(
                        defaults.map(|(_, value)| value),
                        Some(&Yaml::Map(vec![(
                            "run".to_owned(),
                            Yaml::Map(vec![(
                                "shell".to_owned(),
                                Yaml::str(crate::runs_on::CONTAINER_RUN_SHELL),
                            )]),
                        )])),
                        "container job {id} must declare its POSIX shell"
                    );
                } else {
                    assert!(defaults.is_none(), "hosted job {id} gained defaults");
                }
            }
            other => panic!("unexpected runner in {id}: {other:?}"),
        }
    }
    assert!(scale_set >= minimum_per_lane, "missing scale-set jobs");
    assert!(hosted >= minimum_per_lane, "missing hosted jobs");
    if minimum_per_lane == 2 {
        assert_eq!(
            containers, 2,
            "container qualification must cover both lanes"
        );
    }
}

#[test]
fn qualification_and_monitoring_declare_shell_only_for_typed_scale_set() {
    let request = request();
    assert_lane_shells(&qualification(&request).expect("qualification renders"), 2);
    assert_lane_shells(&monitoring(&request).expect("monitoring renders"), 1);
}

#[test]
fn candidate_mbx_roundtrip_defers_gc_and_cleans_after_each_last_consumer() {
    let workflow = render_schema2_workflows(&request())
        .expect("schema2 renders")
        .into_iter()
        .find(|file| file.path == super::QUALIFICATION_WORKFLOW)
        .expect("qualification workflow is emitted");
    let yaml = workflow.bytes.as_str();
    assert_eq!(yaml.matches("MBX_GC_AUTO: \"0\"").count(), 2, "{yaml}");
    assert_eq!(
        yaml.matches("MBX_SHARE_OUT_DIR: \"0\"").count(),
        2,
        "{yaml}"
    );
    assert_eq!(
        yaml.matches("gc-auto-off-final-clean-v1-action-").count(),
        2,
        "writer and reader use the lifecycle-qualified generation: {yaml}"
    );
    assert!(
        yaml.contains("uses: jdx/mr-boxington-action@bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
    );
    assert!(yaml.contains("isolate-objects-cache: \"true\""));
    assert_eq!(
        yaml.matches("if: always() && steps.mbx-ready.outcome == 'success'")
            .count(),
        2,
        "cleanup remains gated by each job's successful action identity check: {yaml}"
    );

    let writer = qualification_job(yaml, "mbx-cache-write-hosted");
    assert_step_before(
        writer,
        "Compile MBX cache probe",
        "Clean MBX workspace outputs",
    );
    assert_step_before(
        writer,
        "Clean MBX workspace outputs",
        "Sample runner disk after cleanup",
    );
    let reader = qualification_job(yaml, "mbx-cache-read-hosted");
    assert_step_before(
        reader,
        "Require imported MBX objects",
        "Compile MBX cache probe",
    );
    assert_step_before(
        reader,
        "Require reused compilation",
        "Clean MBX workspace outputs",
    );
}

fn qualification_job<'a>(yaml: &'a str, id: &str) -> &'a str {
    let start = yaml
        .find(&format!("\n  {id}:\n"))
        .unwrap_or_else(|| panic!("qualification job {id} missing"));
    let body = &yaml[start..];
    let header_end = body[1..].find('\n').map_or(body.len(), |offset| offset + 1);
    let next_job = body[header_end..]
        .match_indices("\n  ")
        .find_map(|(offset, _)| {
            let line = &body[header_end + offset + 3..];
            (!line.starts_with(' ')).then_some(header_end + offset)
        });
    let end = next_job.unwrap_or(body.len());
    &body[..end]
}

fn assert_step_before(job: &str, first: &str, second: &str) {
    let first_at = job
        .find(first)
        .unwrap_or_else(|| panic!("qualification step {first:?} missing from job:\n{job}"));
    let second_at = job
        .find(second)
        .unwrap_or_else(|| panic!("qualification step {second:?} missing from job:\n{job}"));
    assert!(first_at < second_at, "{first:?} must precede {second:?}");
}

#[test]
fn empty_cache_key_templates_bind_run_attempt_in_rendered_workflow() {
    let workflow = render_schema2_workflows(&request())
        .expect("schema2 renders")
        .into_iter()
        .find(|file| file.path == super::QUALIFICATION_WORKFLOW)
        .expect("qualification workflow is emitted");
    let rendered = workflow.bytes.as_str();
    let empty = "g4-empty-${{ github.run_id }}-${{ github.run_attempt }}-${{ github.job }}";
    let space = "g4-space-${{ github.run_id }}-${{ github.run_attempt }}-${{ github.job }}";
    assert_eq!(rendered.matches(&format!("key: {empty}")).count(), 2);
    assert_eq!(rendered.matches(&format!("key: {space}")).count(), 4);

    for job in ["empty-cache-hosted", "empty-cache-scale-set"] {
        for template in [empty, space] {
            let key_for_attempt = |attempt: &str| {
                template
                    .replace("${{ github.run_id }}", "37200000000")
                    .replace("${{ github.run_attempt }}", attempt)
                    .replace("${{ github.job }}", job)
            };
            assert_ne!(
                key_for_attempt("1"),
                key_for_attempt("2"),
                "{job} must not reuse {template} across attempts"
            );
        }
    }
}

#[test]
fn public_hosted_label_rejects_scale_set_tokens_and_unknown_labels() {
    let mut scale_token = request();
    scale_token.hosted_label = scale_token.scale_set.token();
    scale_token.workflows.clear();
    assert!(render_schema2_workflows(&scale_token).is_err_and(|err| {
        err.to_string()
            .contains("schema2_hosted_runner_not_catalog")
    }));

    let mut unknown_label = request();
    unknown_label.hosted_label = "ubuntu-99.99".to_owned();
    assert!(render_schema2_workflows(&unknown_label).is_err_and(|err| {
        err.to_string()
            .contains("schema2_hosted_runner_not_catalog")
    }));
}

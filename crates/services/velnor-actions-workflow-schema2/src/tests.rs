use std::collections::BTreeSet;

use velnor_actions_contract_config::{RoutingWorkflow, SCALE_SET_NAME, VELNOR_LABEL};

use super::render_schema2_workflows;
use super::workflows::{monitoring, qualification};
use velnor_actions_workflow_generator::{MbxQualificationPins, Schema2WorkflowRequest};
use velnor_actions_workflow_steps::setup::MiseSetup;
use velnor_actions_workflow_tree::yaml::Yaml;

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
                velnor_actions_workflow_cache::cache_steps::MBX_ACTION_NAME,
                "b".repeat(40)
            ),
            mbx_version: "1.0.0".to_owned(),
            rust_version: "1.98.1".to_owned(),
        }),
        generator_release: None,
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
            velnor_actions_workflow_tree::runs_on::CONTAINER_RUN_SHELL
        } else {
            velnor_actions_workflow_tree::runs_on::SCALE_SET_RUN_SHELL
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
            Yaml::Str(label) if label == "ubuntu-26.04" => {
                hosted += 1;
                if has_container {
                    assert_eq!(
                        defaults.map(|(_, value)| value),
                        Some(&Yaml::Map(vec![(
                            "run".to_owned(),
                            Yaml::Map(vec![(
                                "shell".to_owned(),
                                Yaml::str(
                                    velnor_actions_workflow_tree::runs_on::CONTAINER_RUN_SHELL
                                ),
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

use std::collections::BTreeSet;

use velnor_actions_contract_config::{RoutingWorkflow, SCALE_SET_NAME, VELNOR_LABEL};

use super::render_schema2_workflows;
use super::workflows::{monitoring, qualification};
use velnor_actions_workflow_generator::{
    GeneratorReleasePins, MbxQualificationPins, ProductReleaseFamily, Schema2WorkflowRequest,
};
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

fn generator_release_pins() -> GeneratorReleasePins {
    let setup = MiseSetup {
        uses: format!("jdx/mise-action@{}", "a".repeat(40)),
        version: "2026.9.18".to_owned(),
        sha256: "b".repeat(64),
    };
    GeneratorReleasePins {
        linux_x86_64_setup: setup.clone(),
        macos_arm64_setup: setup.clone(),
        macos_x86_64_setup: setup,
        install_gate_tools_argv: vec!["mise".to_owned(), "install".to_owned()],
        install_build_tools_argv: vec!["mise".to_owned(), "install".to_owned()],
        install_gh_argv: [
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "install",
            "gh@2.102.0",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        build_argv: vec!["mise".to_owned(), "exec".to_owned()],
        actionlint_argv: vec!["mise".to_owned(), "exec".to_owned()],
        zizmor_argv: vec!["mise".to_owned(), "exec".to_owned()],
        gh_argv: [
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "gh@2.102.0",
            "--",
            "gh",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        rust_version: "1.98.1".to_owned(),
        mr_boxington_version: "1.21.1".to_owned(),
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
    let mut hosted_containers = 0;
    let mut scale_set_containers = 0;
    for (id, job) in jobs {
        let Yaml::Map(fields) = job else {
            panic!("job {id} is not a mapping");
        };
        let Some((_, runs_on)) = fields.iter().find(|(key, _)| key == "runs-on") else {
            panic!("job {id} has no runs-on");
        };
        let defaults = fields.iter().find(|(key, _)| key == "defaults");
        let has_container = fields.iter().any(|(key, _)| key == "container");
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
                if has_container {
                    scale_set_containers += 1;
                }
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
                    hosted_containers += 1;
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
        assert!(hosted_containers > 0, "missing hosted container job");
        assert!(scale_set_containers > 0, "missing scale-set container job");
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

#[test]
fn release_routes_build_a_canonical_typed_family_selection() {
    let mut request = request();
    request.workflows = BTreeSet::from([
        RoutingWorkflow::GeneratorRelease,
        RoutingWorkflow::ImageRelease,
        RoutingWorkflow::MacosBinaryRelease,
    ]);

    let spec = request
        .product_release_spec()
        .expect("valid release selectors")
        .expect("release families selected");
    assert_eq!(
        spec.families(),
        &[
            ProductReleaseFamily::Images,
            ProductReleaseFamily::Binary,
            ProductReleaseFamily::Generator,
        ]
    );
}

#[test]
fn empty_release_selection_keeps_nonrelease_requests_empty() {
    let mut request = request();
    request.workflows = BTreeSet::from([RoutingWorkflow::Qualification]);

    assert!(
        request
            .product_release_spec()
            .expect("non-release selection is valid")
            .is_none()
    );
}

#[test]
fn typed_family_router_emits_existing_outputs_once() {
    let mut request = request();
    request.workflows = BTreeSet::from([
        RoutingWorkflow::ImageRelease,
        RoutingWorkflow::MacosBinaryRelease,
    ]);

    let files = render_schema2_workflows(&request).expect("release workflows render");
    let paths = files.into_iter().map(|file| file.path).collect::<Vec<_>>();
    assert_eq!(
        paths,
        [
            ".github/workflows/image-release.yml",
            ".github/workflows/macos-binary-release.yml",
        ]
    );
}

#[test]
fn typed_generator_route_preserves_missing_pins_failure() {
    let mut request = request();
    request.workflows = BTreeSet::from([RoutingWorkflow::GeneratorRelease]);

    assert!(
        render_schema2_workflows(&request)
            .is_err_and(|error| { error.to_string().contains("generator_release_pins_missing") })
    );
}

#[test]
fn typed_generator_route_emits_the_existing_workflow_and_actions_once() {
    let mut request = request();
    request.workflows = BTreeSet::from([RoutingWorkflow::GeneratorRelease]);
    request.generator_release = Some(generator_release_pins());

    let paths = render_schema2_workflows(&request)
        .expect("generator release renders")
        .into_iter()
        .map(|file| file.path)
        .collect::<Vec<_>>();
    assert_eq!(
        paths,
        [
            ".github/workflows/generator-release.yml",
            ".github/actions/generator-release-build-linux/action.yml",
            ".github/actions/generator-release-qualify-linux/action.yml",
            ".github/actions/generator-release-attest-linux/action.yml",
            ".github/actions/generator-release-build-macos/action.yml",
            ".github/actions/generator-release-qualify-macos/action.yml",
            ".github/actions/generator-release-attest-macos/action.yml",
            ".github/actions/generator-release-build-macos-intel/action.yml",
            ".github/actions/generator-release-qualify-macos-intel/action.yml",
            ".github/actions/generator-release-attest-macos-intel/action.yml",
            ".github/actions/generator-release-attest-manifest/action.yml",
            ".github/actions/generator-release-publish/action.yml",
        ]
    );
}

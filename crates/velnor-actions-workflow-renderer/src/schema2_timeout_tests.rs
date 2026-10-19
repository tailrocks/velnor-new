use super::*;
use crate::workflow_routing::WorkflowRoutingPolicy;
use velnor_actions_contract::config::ValidatedImageReference;

#[test]
fn every_schema2_family_renders_only_bounded_executable_jobs() {
    let pins = product_release_test_pins::test_pins();
    let request = Schema2WorkflowRequest {
        version: "0.1.0".to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set().expect("selector"),
        routing: WorkflowRoutingPolicy::configured(
            velnor_actions_contract::ExecutionMode::Hosted,
            Schema2WorkflowRequest::canonical_scale_set().expect("selector"),
            Some(
                ValidatedImageReference::parse(&format!(
                    "ghcr.io/team/job@sha256:{}",
                    "a".repeat(64)
                ))
                .expect("image reference"),
            ),
        ),
        workflows: BTreeSet::from([
            RoutingWorkflow::Qualification,
            RoutingWorkflow::Monitoring,
            RoutingWorkflow::ImageRelease,
            RoutingWorkflow::MacosBinaryRelease,
            RoutingWorkflow::GeneratorRelease,
        ]),
        mbx_qualification: Some(MbxQualificationPins {
            mise_setup: pins.linux_x86_64_setup.clone(),
            candidate_action_uses: format!("jdx/mr-boxington-action@{}", "a".repeat(40)),
            mbx_version: "1.24.1".to_owned(),
            rust_version: "1.99.1".to_owned(),
        }),
        mise_pin_qualification: Some(MisePinQualificationPins {
            linux_x86_64_setup: pins.linux_x86_64_setup.clone(),
            macos_x86_64_setup: pins.macos_x86_64_setup.clone(),
        }),
        product_release: Some(pins),
    };
    let files = render_schema2_workflows(&request).expect("every family is admitted");
    let paths: BTreeSet<_> = files.iter().map(|file| file.path.as_str()).collect();
    assert_eq!(paths.len(), files.len(), "workflow paths must be unique");
    let workflows: Vec<_> = files
        .iter()
        .filter(|file| file.path.starts_with(".github/workflows/"))
        .collect();
    assert_eq!(workflows.len(), 7);
    assert!(
        workflows
            .iter()
            .any(|workflow| workflow.path == ".github/workflows/job-image-candidate.yml")
    );
    for workflow in workflows {
        let timeouts: Vec<_> = workflow
            .bytes
            .lines()
            .filter_map(|line| line.trim().strip_prefix("timeout-minutes: "))
            .collect();
        assert!(!timeouts.is_empty(), "{}", workflow.path);
        for timeout in timeouts {
            let minutes: u16 = timeout.parse().expect("literal timeout");
            assert_eq!(
                minutes,
                velnor_actions_contract::JobTimeout::DEFAULT_MINUTES,
                "{}",
                workflow.path
            );
        }
    }
}

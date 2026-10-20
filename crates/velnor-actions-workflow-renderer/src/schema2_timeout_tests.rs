use super::*;

#[test]
fn every_schema2_family_renders_only_bounded_executable_jobs() {
    let pins = product_release_test_pins::test_pins();
    let request = Schema2WorkflowRequest {
        version: "0.1.0".to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set().expect("selector"),
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
            mbx_version: "1.25.0".to_owned(),
            rust_version: "1.99.1".to_owned(),
        }),
        mise_pin_qualification: Some(MisePinQualificationPins {
            linux_x86_64_setup: pins.linux_x86_64_setup.clone(),
            macos_x86_64_setup: pins.macos_x86_64_setup.clone(),
        }),
        product_release: Some(pins),
    };
    let files = render_schema2_workflows(&request).expect("every family is admitted");
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
        let (_, jobs) = workflow
            .bytes
            .split_once("\njobs:\n")
            .expect("literal jobs map");
        let mut current = None;
        let mut fields = Vec::new();
        for line in jobs.lines() {
            if let Some(id) = line
                .strip_prefix("  ")
                .and_then(|line| line.strip_suffix(':'))
                && !id.starts_with(char::is_whitespace)
            {
                if let Some(previous) = current.replace(id) {
                    assert_job_budget(&workflow.path, previous, &fields);
                    fields.clear();
                }
            } else {
                fields.push(line);
            }
        }
        assert_job_budget(&workflow.path, current.expect("nonempty jobs map"), &fields);
    }
}

fn assert_job_budget(workflow: &str, id: &str, fields: &[&str]) {
    let timeouts: Vec<_> = fields
        .iter()
        .filter_map(|line| line.strip_prefix("    timeout-minutes: "))
        .collect();
    let caller = fields.iter().any(|line| line.starts_with("    uses: "));
    let concrete = fields.iter().any(|line| line.starts_with("    steps:"));
    assert_ne!(
        caller, concrete,
        "{workflow}:{id} must have exactly one job kind"
    );
    if caller {
        assert!(
            timeouts.is_empty(),
            "{workflow}:{id} caller cannot have a timeout"
        );
        return;
    }
    assert_eq!(timeouts.len(), 1, "{workflow}:{id} must have one timeout");
    let minutes: u16 = timeouts[0].parse().expect("literal timeout");
    assert!(velnor_actions_contract::JobTimeout::is_valid_minutes(
        u32::from(minutes)
    ));
    let compiler = matches!(
        (workflow, id),
        (
            ".github/workflows/product-release-binary.yml",
            "build-binary"
        ) | (
            ".github/workflows/product-release-generator.yml",
            "build-linux" | "build-macos" | "build-macos-intel"
        )
    );
    let expected = if compiler { 10 } else { 5 };
    assert_eq!(minutes, expected, "{workflow}:{id}");
}

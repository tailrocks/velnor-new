use super::*;

const MATRIX_ROLE_TEST: &str = "package_release_publish::run_identity::active_role::rendered_tests::preview_acquire_tests::rendered_preview_source_acquire_binds_each_matrix_role";

#[test]
fn rendered_preview_source_acquire_binds_each_matrix_role() {
    if let Ok(expectation) = std::env::var(MATRIX_ROLE_CHILD) {
        assert_matrix_role_child(&expectation);
        return;
    }

    let spec = fixture(PackageReleaseChannel::Preview);
    let plan = spec.policy.resolve("0.6.4".to_owned()).expect("plan");
    let pairs = expected_release_payload_pairs(&plan).expect("declared pairs");
    let rendered = render_consumer_preview(
        &spec,
        &velnor_actions_workflow_renderer::workflow_routing::WorkflowRoutingPolicy::hosted_only(),
    )
    .expect("actual preview renderer");
    let payload_job = job_block(&rendered.bytes, "preview-payload");
    let payload_acquire = operation_blocks(payload_job, "acquire-preview-source-v1")
        .into_iter()
        .next()
        .expect("payload source acquisition step");
    for (key, value) in [
        ("VELNOR_RELEASE_PRODUCT_ID", "${{ matrix.product_id }}"),
        ("VELNOR_RELEASE_TARGET", "${{ matrix.target }}"),
    ] {
        assert!(
            payload_acquire.contains(&format!("{key}: {value}")),
            "missing step-local {key} binding:\n{payload_acquire}"
        );
    }
    assert!(!payload_acquire.contains("VELNOR_RELEASE_STAGE: payload"));
    assert!(!payload_acquire.contains("VELNOR_RELEASE_ADMISSION_DIGEST"));
    assert!(!payload_acquire.contains("VELNOR_RELEASE_POLICY_ADMISSION_DIGEST"));

    for pair in &pairs {
        let resolved = payload_acquire
            .replace("${{ matrix.product_id }}", &pair.product_id)
            .replace("${{ matrix.target }}", &pair.target);
        assert!(resolved.contains(&format!("VELNOR_RELEASE_PRODUCT_ID: {}", pair.product_id)));
        assert!(resolved.contains(&format!("VELNOR_RELEASE_TARGET: {}", pair.target)));
        run_matrix_role_child(Some(&pair.product_id), Some(&pair.target), true);
    }

    let assembly_job = job_block(&rendered.bytes, "preview-assemble");
    let assembly_acquire = operation_blocks(assembly_job, "acquire-preview-source-v1")
        .into_iter()
        .next()
        .expect("assembly source acquisition step");
    assert!(!assembly_acquire.contains("VELNOR_RELEASE_PRODUCT_ID"));
    assert!(!assembly_acquire.contains("VELNOR_RELEASE_TARGET"));
    assert!(!assembly_acquire.contains("VELNOR_RELEASE_STAGE: payload"));
    assert!(!assembly_acquire.contains("VELNOR_RELEASE_ADMISSION_DIGEST"));
    assert!(!assembly_acquire.contains("VELNOR_RELEASE_POLICY_ADMISSION_DIGEST"));

    let first = pairs.first().expect("at least one declared pair");
    run_matrix_role_child(None, Some(&first.target), false);
    run_matrix_role_child(Some(&first.product_id), None, false);
    run_matrix_role_child(Some("foreign-product"), Some(&first.target), false);
}

fn job_block<'a>(rendered: &'a str, id: &str) -> &'a str {
    let marker = format!("  {id}:\n");
    let start = rendered.find(&marker).expect("rendered job");
    let after_job = start + marker.len();
    let mut offset = after_job;
    for line in rendered[after_job..].split_inclusive('\n') {
        if line.starts_with("  ") && !line.starts_with("   ") && line.trim_end().ends_with(':') {
            return &rendered[start..offset];
        }
        offset += line.len();
    }
    &rendered[start..]
}

fn operation_blocks<'a>(rendered: &'a str, operation: &str) -> Vec<&'a str> {
    let marker = format!("VELNOR_INTERNAL_OP: {operation}");
    rendered
        .match_indices(&marker)
        .map(|(at, _)| {
            let start = rendered[..at]
                .rfind("\n      - ")
                .map_or(0, |index| index + 1);
            let end = rendered[at..]
                .find("\n      - ")
                .map_or(rendered.len(), |offset| at + offset + 1);
            &rendered[start..end]
        })
        .collect()
}

fn run_matrix_role_child(product: Option<&str>, target: Option<&str>, accept: bool) {
    let mut command = Command::new(std::env::current_exe().expect("test executable"));
    command
        .args(["--exact", MATRIX_ROLE_TEST, "--nocapture"])
        .env_clear()
        .env(MATRIX_ROLE_CHILD, if accept { "accept" } else { "reject" });
    if let Some(product) = product {
        command.env("VELNOR_RELEASE_PRODUCT_ID", product);
    }
    if let Some(target) = target {
        command.env("VELNOR_RELEASE_TARGET", target);
    }
    let output = command.output().expect("isolated role validator");
    assert!(
        output.status.success(),
        "role child failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("PREVIEW_MATRIX_ROLE_CHILD_EXECUTED"),
        "exact child must execute and print its marker: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

fn assert_matrix_role_child(expectation: &str) {
    let plan = fixture(PackageReleaseChannel::Preview)
        .policy
        .resolve("0.6.4".to_owned())
        .expect("preview plan");
    let role = release_job_name(&plan, "preview-payload");
    if expectation == "accept" {
        let product = std::env::var("VELNOR_RELEASE_PRODUCT_ID").expect("product binding");
        let target = std::env::var("VELNOR_RELEASE_TARGET").expect("target binding");
        assert_eq!(
            role.expect("declared matrix pair"),
            format!("Build {product} / {target}")
        );
    } else {
        assert!(
            role.is_err(),
            "missing or foreign matrix pair must be rejected"
        );
    }
    println!("PREVIEW_MATRIX_ROLE_CHILD_EXECUTED");
}

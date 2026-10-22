//! Literal payload graph and immutable artifact handoff regressions.

use super::*;
use velnor_actions_contract::preview_production::{
    ReleaseProductionContext, release_payload_artifact_name,
};

fn fields(job: &Yaml) -> &[(String, Yaml)] {
    let Yaml::Map(fields) = job else {
        panic!("job map")
    };
    fields
}

fn field<'a>(job: &'a Yaml, key: &str) -> &'a Yaml {
    &fields(job)
        .iter()
        .find(|(name, _)| name == key)
        .expect(key)
        .1
}

#[test]
fn preview_payload_matrix_is_selected_from_authenticated_source_outputs() {
    let spec = fixture();
    let pairs = spec.policy.payload_pairs().expect("pairs");
    assert_eq!(pairs.len(), 6);
    let jobs = production::jobs(&spec).expect("production");
    assert_eq!(jobs.len(), 2);
    let (id, payload) = &jobs[0];
    assert_eq!(id, "preview-payload");
    assert_eq!(
        field(payload, "name"),
        &Yaml::str("Build ${{ matrix.product_id }} / ${{ matrix.target }}")
    );
    assert_eq!(
        field(payload, "timeout-minutes"),
        &Yaml::str("${{ matrix.timeout_minutes }}")
    );
    assert_eq!(field(payload, "needs"), &needs(&["preview-eligibility"]));
    let rendered = crate::yaml::render_yaml(payload);
    assert!(rendered.contains("fail-fast: false"));
    assert!(
        rendered
            .contains("matrix: ${{ fromJSON(needs.preview-eligibility.outputs.payload_matrix) }}")
    );
    assert!(rendered.contains("runs-on: ${{ matrix.runner }}"));
    assert!(rendered.contains("VELNOR_RELEASE_PRODUCT_ID: ${{ matrix.product_id }}"));
    assert!(rendered.contains("VELNOR_RELEASE_TARGET: ${{ matrix.target }}"));
    assert!(!rendered.contains("VELNOR_RELEASE_NEEDS_JSON"));
    let Yaml::Seq(payload_steps) = field(payload, "steps") else {
        panic!("payload steps")
    };
    let executor = payload_steps
        .iter()
        .find(|step| has_operation(step, "execute-release-producer-v1"))
        .expect("payload executor");
    assert_token_free_environment(executor);
    let (id, assembly) = jobs.last().expect("assembly");
    assert_eq!(id, "preview-assemble");
    assert_eq!(field(assembly, "name"), &Yaml::str(id));
    assert_eq!(
        field(assembly, "needs"),
        &needs(&["preview-eligibility", "preview-payload"])
    );
    assert_eq!(
        field(assembly, "permissions"),
        &permissions(&[("actions", "read"), ("contents", "read")])
    );
    let rendered = crate::yaml::render_yaml(assembly);
    assert!(rendered.contains("VELNOR_RELEASE_STAGE: assemble"));
    assert!(rendered.contains("needs.preview-eligibility.outputs.producer_runner"));
    assert!(!rendered.contains("VELNOR_RELEASE_NEEDS_JSON"));
    assert!(!rendered.contains("id-token:"));
    assert!(!rendered.contains("environment:"));
}

#[test]
fn generated_leaf_names_match_native_admission_without_name_downloads() {
    let spec = fixture();
    let context = ReleaseProductionContext {
        repository_id: 42,
        head_repository_id: 42,
        source_sha: "a".repeat(40),
        source_ref: "refs/heads/main".to_owned(),
        workflow_authority_sha: "a".repeat(40),
        run_id: 123,
        run_attempt: 2,
        ci_run_id: 99,
        ci_run_attempt: 1,
        commit_count: 100,
        version: "1.2.3-preview.100+aaaaaaa".to_owned(),
    };
    for pair in spec.policy.payload_pairs().expect("pairs") {
        let upload = production::upload(Some(&pair), false);
        let with = field(&upload, "with");
        let Yaml::Str(name) = field(with, "name") else {
            panic!("artifact name")
        };
        let actual = name
            .replace("${{ github.run_id }}", "123")
            .replace("${{ github.run_attempt }}", "2");
        assert_eq!(
            actual,
            release_payload_artifact_name(
                &spec
                    .policy
                    .resolve("1.2.3".to_owned())
                    .expect("resolved fixture"),
                &pair,
                &context
            )
            .expect("native name")
        );
        assert_eq!(field(&upload, "id"), &Yaml::str("upload"));
        assert_eq!(field(with, "overwrite"), &Yaml::Bool(false));
    }
    let rendered = render_consumer_preview(&spec).expect("render").bytes;
    let signer = job(&rendered, "preview-signer", Some("preview-publisher"));
    assert!(signer.contains("admit-release-aggregate-v1"));
    assert_eq!(signer.matches("id: aggregate\n").count(), 1);
    assert_eq!(signer.matches("id: release\n").count(), 1);
    assert!(signer.contains("needs.preview-assemble.outputs.artifact_id"));
    assert!(signer.contains("needs.preview-assemble.outputs.artifact_digest"));
    assert!(!signer.contains("actions/download-artifact"));
    assert!(!rendered.contains("preview-payload-000:"));
}

#[test]
fn production_plan_is_exact_job_environment_and_not_repeated_in_operation() {
    let spec = fixture();
    for (id, job) in production::jobs(&spec).expect("production") {
        assert_eq!(
            field(&job, "env"),
            &Yaml::Map(vec![(
                "VELNOR_RELEASE_POLICY".to_owned(),
                Yaml::str("${{ needs.preview-eligibility.outputs.release_policy }}")
            )])
        );
        let Yaml::Seq(steps) = field(&job, "steps") else {
            panic!("steps")
        };
        let operations = steps
            .iter()
            .filter(|step| {
                fields(step).iter().any(|(key, value)| {
                    key == "env" && crate::yaml::render_yaml(value).contains("VELNOR_INTERNAL_OP:")
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(
            operations.len(),
            if id == "preview-assemble" { 3 } else { 2 }
        );
        for operation in operations {
            assert!(
                !fields(field(operation, "env"))
                    .iter()
                    .any(|(key, _)| key == "VELNOR_RELEASE_POLICY")
            );
            assert!(crate::yaml::render_yaml(operation).contains("VELNOR_REQUEST_FILE:"));
        }
    }
}

#[test]
fn maximum_literal_payload_inventory_renders_within_workflow_limit() {
    let mut spec = fixture();
    let template = spec.policy.products[1].clone();
    spec.policy.products = (0..32)
        .map(|index| {
            let mut product = template.clone();
            product.id = format!("product-{index:03}");
            product.package = product.id.clone();
            product.bins = vec![product.id.clone()];
            product
        })
        .collect();
    bind_version_owners(&mut spec);
    let pairs = spec.policy.payload_pairs().expect("maximum admitted plan");
    assert_eq!(pairs.len(), 128);
    let rendered = render_consumer_preview(&spec)
        .expect("maximum graph renders")
        .bytes;
    assert!(
        rendered.len() <= crate::workflow_size::MAX_WORKFLOW_BYTES,
        "{} bytes",
        rendered.len()
    );
    assert_eq!(rendered.matches("  preview-payload:\n").count(), 1);
    assert_eq!(rendered.matches("  preview-payload-000:\n").count(), 0);
    let jobs = production::jobs(&spec).expect("matrix graph");
    assert_eq!(jobs.len(), 2);
    for (_, job) in &jobs {
        assert_eq!(
            field(job, "env"),
            &Yaml::Map(vec![(
                "VELNOR_RELEASE_POLICY".to_owned(),
                Yaml::str("${{ needs.preview-eligibility.outputs.release_policy }}"),
            )])
        );
    }
    let Yaml::Seq(dependencies) = field(&jobs.last().expect("assembly").1, "needs") else {
        panic!("needs")
    };
    assert_eq!(dependencies.len(), 2);
}

#[test]
fn irreducible_preview_workflow_still_fails_the_unchanged_byte_limit() {
    let mut spec = fixture();
    // One literal argument alone exceeds the workflow limit even after all
    // repeated job fields and scripts have been shared by existing machinery.
    spec.install_rust_argv
        .push("x".repeat(crate::workflow_size::MAX_WORKFLOW_BYTES + 1));
    let error = render_consumer_preview(&spec).expect_err("irreducible graph rejected");
    assert!(
        matches!(error, RenderError::InvalidWorkflow(problem) if problem.starts_with("workflow_too_large:") && problem.ends_with(":500000"))
    );
}

#[test]
fn assembly_acquisition_exits_before_any_checkout_or_source_preparation() {
    let spec = fixture();
    let jobs = production::jobs(&spec).expect("jobs");
    let (_, assembly) = jobs.last().expect("assembly");
    let Yaml::Seq(steps) = field(assembly, "steps") else {
        panic!("steps")
    };
    let admission = steps
        .iter()
        .position(|step| {
            fields(step)
                .iter()
                .any(|(key, value)| key == "id" && *value == Yaml::str("acquire"))
        })
        .expect("acquisition");
    assert_assembly_bootstrap(&spec, steps, admission);
    let acquired = &steps[admission];
    assert!(has_operation(acquired, "acquire-release-leaves-v1"));
    assert_assembly_bindings(acquired);
    assert_eq!(
        env_value(acquired, "GH_TOKEN"),
        Some(&Yaml::str("${{ github.token }}"))
    );
    assert_after_assembly_acquisition(steps, admission);
    assert_executor_step(steps);
}

fn assert_assembly_bootstrap(spec: &ConsumerReleaseSpec, steps: &[Yaml], admission: usize) {
    assert_eq!(
        admission, 9,
        "pinned setup, target-specific helper bootstrap and source admission precede leaf acquisition"
    );
    let gh_setup = steps
        .iter()
        .position(|step| {
            fields(step).iter().any(|(key, value)| {
                key == "name" && *value == Yaml::str("Install pinned GitHub CLI")
            })
        })
        .expect("pinned GitHub CLI installation");
    assert_eq!(
        field(&steps[gh_setup], "run"),
        &Yaml::str(join_argv_for_run(&spec.install_gh_argv).expect("pinned GitHub install"))
    );
    for step in &steps[..admission] {
        let rendered = crate::yaml::render_yaml(step);
        assert!(!rendered.contains("checkout"));
        if has_operation(step, "acquire-preview-source-v1") {
            assert_eq!(
                env_value(step, "GH_TOKEN"),
                Some(&Yaml::str("${{ github.token }}"))
            );
        } else {
            assert_token_free_environment(step);
        }
        assert!(!rendered.contains("mise run"));
    }
}

fn assert_after_assembly_acquisition(steps: &[Yaml], admission: usize) {
    let checkout = steps
        .iter()
        .position(|step| {
            fields(step).iter().any(|(key, value)| {
                key == "uses" && *value == Yaml::str(crate::steps::CHECKOUT_USES)
            })
        })
        .expect("checkout");
    assert!(checkout > admission);
    for step in &steps[admission + 1..] {
        assert_token_free_environment(step);
        assert!(!has_operation(step, "acquire-release-leaves-v1"));
        assert!(!has_operation(step, "admit-release-aggregate-v1"));
    }
}

fn assert_executor_step(steps: &[Yaml]) {
    let executor = steps
        .iter()
        .find(|step| has_operation(step, "execute-release-producer-v1"))
        .expect("executor");
    assert_assembly_bindings(executor);
    assert_eq!(
        env_value(executor, "VELNOR_RELEASE_POLICY_ADMISSION_DIGEST"),
        Some(&Yaml::str(
            "${{ steps.policy-admission.outputs.admission_digest }}"
        ))
    );
    let Yaml::Str(run) = field(executor, "run") else {
        panic!("executor run")
    };
    assert!(run.starts_with(&crate::toolchain_env::credential_unset_prelude()));
    assert!(
        !run.contains("printf '%s'"),
        "executor must retain acquired plan"
    );
}

fn assert_token_free_environment(step: &Yaml) {
    if let Some((_, Yaml::Map(environment))) = fields(step).iter().find(|(key, _)| key == "env") {
        for (key, value) in environment {
            if matches!(
                key.as_str(),
                "GH_TOKEN" | "GITHUB_TOKEN" | "VELNOR_RELEASE_STATUS_TOKEN"
            ) {
                assert_eq!(value, &Yaml::str(""), "nonempty credential in {step:?}");
            }
        }
    }
}

fn env_value<'a>(step: &'a Yaml, key: &str) -> Option<&'a Yaml> {
    let (_, Yaml::Map(environment)) = fields(step).iter().find(|(name, _)| name == "env")? else {
        return None;
    };
    environment
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
}

fn has_operation(step: &Yaml, operation: &str) -> bool {
    env_value(step, "VELNOR_INTERNAL_OP") == Some(&Yaml::str(operation))
}

fn assert_assembly_bindings(step: &Yaml) {
    assert_eq!(
        env_value(step, "VELNOR_RELEASE_STAGE"),
        Some(&Yaml::str("assemble"))
    );
    assert!(env_value(step, "VELNOR_RELEASE_NEEDS_JSON").is_none());
    for key in [
        "VELNOR_RELEASE_SOURCE_ADMISSION_ARTIFACT_ID",
        "VELNOR_RELEASE_SOURCE_ADMISSION_ARTIFACT_DIGEST",
        "VELNOR_RELEASE_SOURCE_ADMISSION_SHA256",
    ] {
        assert!(env_value(step, key).is_some(), "missing {key}");
    }
    assert!(env_value(step, "VELNOR_RELEASE_PRODUCT_ID").is_none());
    assert!(env_value(step, "VELNOR_RELEASE_TARGET").is_none());
}

#[path = "schema2_consumer_preview_payload_display_tests.rs"]
mod display_tests;

fn bind_version_owners(spec: &mut ConsumerReleaseSpec) {
    spec.policy.version.version_package = spec.policy.products[0].package.clone();
    spec.policy.version.bindings = spec
        .policy
        .products
        .iter()
        .map(|product| velnor_actions_contract::CargoVersionBinding {
            package: product.package.clone(),
            manifest_path: format!("{}/Cargo.toml", product.package),
            workspace_manifest_path: "Cargo.toml".to_owned(),
        })
        .collect();
    spec.policy
        .version
        .bindings
        .sort_by(|left, right| left.package.cmp(&right.package));
}

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
fn every_literal_pair_has_exact_owner_and_complete_assembly_dependencies() {
    let spec = fixture();
    let pairs = spec.policy.payload_pairs().expect("pairs");
    assert_eq!(pairs.len(), 6);
    let jobs = production::jobs(&spec).expect("production");
    assert_eq!(jobs.len(), 7);
    let mut expected = vec![Yaml::str("preview-eligibility")];
    for ((id, job), pair) in jobs.iter().zip(&pairs) {
        assert_eq!(*id, spec.policy.payload_job_id(pair).expect("owner"));
        assert_eq!(
            field(job, "name"),
            &Yaml::str(format!("Build {} / {}", pair.product_id, pair.target))
        );
        assert_eq!(field(job, "timeout-minutes"), &Yaml::Int(10));
        assert_eq!(field(job, "needs"), &needs(&["preview-eligibility"]));
        assert_eq!(
            field(job, "permissions"),
            &permissions(&[("actions", "read"), ("contents", "read")])
        );
        expected.push(Yaml::str(id));
        let rendered = crate::yaml::render_yaml(job);
        let Yaml::Seq(steps) = field(job, "steps") else {
            panic!("steps")
        };
        let executor = steps
            .iter()
            .find(|step| has_operation(step, "execute-release-producer-v1"))
            .expect("payload executor");
        assert_eq!(
            env_value(executor, "VELNOR_RELEASE_PRODUCT_ID"),
            Some(&Yaml::str(&pair.product_id))
        );
        assert_eq!(
            env_value(executor, "VELNOR_RELEASE_TARGET"),
            Some(&Yaml::str(&pair.target))
        );
        assert_eq!(
            env_value(executor, "VELNOR_RELEASE_STAGE"),
            Some(&Yaml::str("payload"))
        );

        assert_token_free_environment(executor);
        assert!(!rendered.contains("matrix:"));
    }
    let (id, assembly) = jobs.last().expect("assembly");
    assert_eq!(id, "preview-assemble");
    assert_eq!(field(assembly, "name"), &Yaml::str(id));
    assert_eq!(field(assembly, "needs"), &Yaml::Seq(expected));
    assert_eq!(
        field(assembly, "permissions"),
        &permissions(&[("actions", "read"), ("contents", "read")])
    );
    let rendered = crate::yaml::render_yaml(assembly);
    assert!(rendered.contains("VELNOR_RELEASE_STAGE: assemble"));
    assert!(rendered.contains("${{ toJSON(needs) }}"));
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
        let upload = production::upload(Some(&pair));
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
    assert!(!rendered.contains("preview-producer:"));
}

#[test]
fn production_plan_is_exact_job_environment_and_not_repeated_in_operation() {
    let spec = fixture();
    let plan = String::from_utf8(spec.policy.canonical_bytes().expect("full plan")).expect("utf8");
    for (id, job) in production::jobs(&spec).expect("production") {
        assert_eq!(
            field(&job, "env"),
            &Yaml::Map(vec![("VELNOR_RELEASE_POLICY".to_owned(), Yaml::str(&plan))])
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
    assert!(
        rendered.matches("VELNOR_RELEASE_POLICY:").count() <= 5,
        "public plan must use existing mapping sharing"
    );
    for pair in &pairs {
        let id = spec.policy.payload_job_id(pair).expect("owner");
        assert_eq!(rendered.matches(&format!("  {id}:\n")).count(), 1);
    }
    let jobs = production::jobs(&spec).expect("literal graph");
    assert_eq!(jobs.len(), 129);
    let Yaml::Seq(dependencies) = field(&jobs.last().expect("assembly").1, "needs") else {
        panic!("needs")
    };
    assert_eq!(dependencies.len(), 129);
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
    assert_eq!(
        admission, 4,
        "pinned setup, generator bootstrap and authenticated policy admission precede leaf acquisition"
    );
    assert_eq!(
        field(&steps[1], "run"),
        &Yaml::str(join_argv_for_run(&spec.install_gh_argv).expect("pinned GitHub install"))
    );
    for step in &steps[..admission] {
        let rendered = crate::yaml::render_yaml(step);
        assert!(!rendered.contains("checkout"));
        if has_operation(step, "admit-release-policy-v1") {
            assert_eq!(
                env_value(step, "GH_TOKEN"),
                Some(&Yaml::str("${{ github.token }}"))
            );
        } else {
            assert_token_free_environment(step);
        }
        assert!(!rendered.contains("mise run"));
    }
    let acquired = &steps[admission];
    assert!(has_operation(acquired, "acquire-release-leaves-v1"));
    assert_assembly_bindings(acquired);
    assert_eq!(
        env_value(acquired, "GH_TOKEN"),
        Some(&Yaml::str("${{ github.token }}"))
    );
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
    let executor = steps
        .iter()
        .find(|step| has_operation(step, "execute-release-producer-v1"))
        .expect("executor");
    assert_assembly_bindings(executor);
    assert_eq!(
        env_value(executor, "VELNOR_RELEASE_ADMISSION_DIGEST"),
        Some(&Yaml::str("${{ steps.acquire.outputs.admission_digest }}"))
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
            if matches!(key.as_str(), "GH_TOKEN" | "GITHUB_TOKEN") {
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
    assert_eq!(
        env_value(step, "VELNOR_RELEASE_NEEDS_JSON"),
        Some(&Yaml::str("${{ toJSON(needs) }}"))
    );
    assert!(env_value(step, "VELNOR_RELEASE_PRODUCT_ID").is_none());
    assert!(env_value(step, "VELNOR_RELEASE_TARGET").is_none());
}

#[test]
fn jackin_payload_display_names_describe_all_six_literal_pairs() {
    let mut spec = fixture();
    let mut jackin = spec.policy.products[1].clone();
    jackin.id = "jackin".to_owned();
    jackin.package = "jackin".to_owned();
    jackin.bins = ["jackin", "jackin-role", "jackin-usage-broker"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    let mut capsule = spec.policy.products[0].clone();
    capsule.id = "jackin-capsule".to_owned();
    capsule.package = "jackin-capsule".to_owned();
    capsule.bins = vec!["jackin-capsule".to_owned()];
    capsule.targets = ["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    spec.policy.products = vec![jackin, capsule];
    bind_version_owners(&mut spec);
    let expected = [
        "Build jackin / aarch64-apple-darwin",
        "Build jackin / aarch64-unknown-linux-gnu",
        "Build jackin / x86_64-apple-darwin",
        "Build jackin / x86_64-unknown-linux-gnu",
        "Build jackin-capsule / aarch64-unknown-linux-gnu",
        "Build jackin-capsule / x86_64-unknown-linux-gnu",
    ];
    let jobs = production::jobs(&spec).expect("production");
    assert_eq!(jobs.len(), 7);
    for (index, name) in expected.iter().enumerate() {
        let (id, job) = &jobs[index];
        assert_eq!(id, &format!("preview-payload-{index:03}"));
        assert_eq!(field(job, "name"), &Yaml::str(*name));
    }
    let rendered = render_consumer_preview(&spec).expect("preview");
    for name in expected {
        assert!(rendered.bytes.contains(&format!("name: {name}\n")));
    }
    let (_, assembly) = jobs.last().expect("assembly");
    assert_eq!(field(assembly, "name"), &Yaml::str("preview-assemble"));
    assert!(rendered.bytes.contains("name: preview-signer\n"));
}

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

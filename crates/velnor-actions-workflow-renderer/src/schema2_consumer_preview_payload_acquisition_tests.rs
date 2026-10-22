//! Preview assembly acquisition order and credential-boundary regressions.

use super::*;

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

use velnor_actions_contract::ReleaseTarget;

use crate::yaml::Yaml;

use super::super::super::features::CHECKOUT_USES;
use super::super::super::generator_release;
use super::compose_job;

fn job(needs: &[&str]) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Generator job")),
        ("runs-on".to_owned(), Yaml::str("ubuntu-26.04")),
        (
            "needs".to_owned(),
            Yaml::Seq(needs.iter().map(|need| Yaml::str(*need)).collect()),
        ),
        (
            "env".to_owned(),
            Yaml::Map(vec![("EXISTING".to_owned(), Yaml::str("value"))]),
        ),
        (
            "steps".to_owned(),
            Yaml::Seq(vec![Yaml::Map(vec![
                ("name".to_owned(), Yaml::str("Checkout")),
                ("uses".to_owned(), Yaml::str(CHECKOUT_USES)),
                (
                    "with".to_owned(),
                    Yaml::Map(vec![("persist-credentials".to_owned(), Yaml::Bool(false))]),
                ),
            ])]),
        ),
    ])
}

fn attestation_job(id: &str, build: &str, qualify: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Attest target")),
        ("runs-on".to_owned(), Yaml::str("ubuntu-26.04")),
        (
            "needs".to_owned(),
            Yaml::Seq([build, qualify].map(Yaml::str).to_vec()),
        ),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![
                ("actions".to_owned(), Yaml::str("read")),
                ("attestations".to_owned(), Yaml::str("write")),
                ("contents".to_owned(), Yaml::str("read")),
                ("id-token".to_owned(), Yaml::str("write")),
            ]),
        ),
        (
            "steps".to_owned(),
            Yaml::Seq(vec![
                Yaml::Map(vec![
                    ("name".to_owned(), Yaml::str("Checkout")),
                    ("uses".to_owned(), Yaml::str(CHECKOUT_USES)),
                    (
                        "with".to_owned(),
                        Yaml::Map(vec![("persist-credentials".to_owned(), Yaml::Bool(false))]),
                    ),
                ]),
                Yaml::Map(vec![
                    ("name".to_owned(), Yaml::str(format!("Attest {id}"))),
                    (
                        "uses".to_owned(),
                        Yaml::str(format!("./.github/actions/{id}")),
                    ),
                    (
                        "with".to_owned(),
                        Yaml::Map(vec![(
                            "artifact_id".to_owned(),
                            Yaml::str(format!("${{{{ needs.{build}.outputs.artifact_id }}}}")),
                        )]),
                    ),
                ]),
            ]),
        ),
    ])
}

#[test]
fn roles_bind_each_target_to_its_build_qualification_and_attestation_nodes() {
    assert_eq!(
        generator_release::job_role("verify-release-source"),
        Some(generator_release::JobRole::SourceGate)
    );
    for (target, build, qualify, attest) in [
        (
            ReleaseTarget::LinuxX86_64,
            "build-linux",
            "qualify-linux",
            "attest-linux",
        ),
        (
            ReleaseTarget::MacosArm64,
            "build-macos",
            "qualify-macos",
            "attest-macos",
        ),
        (
            ReleaseTarget::MacosX86_64,
            "build-macos-intel",
            "qualify-macos-intel",
            "attest-macos-intel",
        ),
    ] {
        assert_eq!(
            generator_release::job_role(build),
            Some(generator_release::JobRole::Build(target))
        );
        assert_eq!(
            generator_release::job_role(qualify),
            Some(generator_release::JobRole::Qualify(target))
        );
        assert_eq!(
            generator_release::job_role(attest),
            Some(generator_release::JobRole::Attest(target))
        );
    }
    assert_eq!(
        generator_release::job_role("candidate-manifest"),
        Some(generator_release::JobRole::CandidateManifest)
    );
    assert_eq!(
        generator_release::job_role("attest-manifest"),
        Some(generator_release::JobRole::AttestManifest)
    );
    assert_eq!(
        generator_release::job_role("publish-generator"),
        Some(generator_release::JobRole::Publish)
    );
    assert_eq!(generator_release::job_role("attest-generator-assets"), None);
    assert_eq!(generator_release::job_role("publish-unknown"), None);
}

#[test]
fn source_gate_remains_in_the_called_family_before_candidate_builds() {
    let (id, Yaml::Map(fields)) = compose_job("verify-release-source".to_owned(), job(&[]))
        .expect("source gate has a typed role")
    else {
        panic!("source gate is a job map");
    };
    assert_eq!(id, "verify-release-source");
    assert!(fields.iter().any(|(key, value)| {
        key == "needs"
            && matches!(value, Yaml::Seq(needs) if needs.contains(&Yaml::str("verify-release-caller")))
    }));
    assert!(fields.iter().any(|(key, value)| {
        key == "if" && value == &Yaml::str("inputs.release_action == 'build'")
    }));
}

#[test]
fn composed_publish_requires_the_verified_manifest_and_all_target_attestations() {
    let (id, Yaml::Map(fields)) = compose_job("publish-generator".to_owned(), job(&[]))
        .expect("typed generator publisher composes")
    else {
        panic!("publisher is a map");
    };
    assert_eq!(id, "publish-generator");
    let needs = fields
        .iter()
        .find(|(key, _)| key == "needs")
        .map(|(_, value)| value)
        .expect("publisher needs");
    let Yaml::Seq(needs) = needs else {
        panic!("publisher needs are a sequence");
    };
    for needed in [
        "attest-linux",
        "attest-macos",
        "attest-macos-intel",
        "attest-manifest",
    ] {
        assert!(needs.contains(&Yaml::str(needed)), "missing need {needed}");
    }
    let condition = fields
        .iter()
        .find(|(key, _)| key == "if")
        .map(|(_, value)| value)
        .expect("publisher condition");
    assert_eq!(
        condition,
        &Yaml::str(
            "always() && inputs.release_action == 'build' && needs.attest-linux.result == 'success' && needs.attest-macos.result == 'success' && needs.attest-macos-intel.result == 'success' && needs.attest-manifest.result == 'success'"
        )
    );
}

#[test]
fn each_target_attester_keeps_its_own_artifact_and_qualification_dependency() {
    for (id, build, qualify) in [
        ("attest-linux", "build-linux", "qualify-linux"),
        ("attest-macos", "build-macos", "qualify-macos"),
        (
            "attest-macos-intel",
            "build-macos-intel",
            "qualify-macos-intel",
        ),
    ] {
        let (actual_id, Yaml::Map(fields)) =
            compose_job(id.to_owned(), attestation_job(id, build, qualify))
                .expect("typed target attestation composes")
        else {
            panic!("target attester is a map");
        };
        assert_eq!(actual_id, id);
        let needs = fields
            .iter()
            .find(|(key, _)| key == "needs")
            .map(|(_, value)| value)
            .expect("attester needs");
        let Yaml::Seq(needs) = needs else {
            panic!("attester needs are a sequence");
        };
        for required in ["verify-release-caller", build, qualify] {
            assert!(needs.contains(&Yaml::str(required)), "missing {required}");
        }
        assert!(!needs.contains(&Yaml::str("contents")));
        let permissions = fields
            .iter()
            .find(|(key, _)| key == "permissions")
            .map(|(_, value)| value)
            .expect("attester permissions");
        let Yaml::Map(permission_entries) = permissions else {
            panic!("attester permissions are a map");
        };
        assert!(permission_entries.contains(&("id-token".to_owned(), Yaml::str("write"))));
        assert!(permission_entries.contains(&("contents".to_owned(), Yaml::str("read"))));
        assert!(!permission_entries.contains(&("contents".to_owned(), Yaml::str("write"))));
        let steps = fields
            .iter()
            .find(|(key, _)| key == "steps")
            .map(|(_, value)| value)
            .expect("attester steps");
        let Yaml::Seq(steps) = steps else {
            panic!("attester steps are a sequence");
        };
        let Yaml::Map(step) = &steps[1] else {
            panic!("target attestation is a step");
        };
        assert!(step.iter().any(|(key, value)| {
            key == "uses" && value == &Yaml::str(format!("./.github/actions/{id}"))
        }));
        assert!(step.iter().any(|(key, value)| {
            key == "with"
                && matches!(value, Yaml::Map(inputs) if inputs.contains(&(                    "artifact_id".to_owned(),                    Yaml::str(format!("${{{{ needs.{build}.outputs.artifact_id }}}}"))                )))
        }));
    }
}

#[test]
fn generator_build_is_bound_to_prepared_source_and_typed_target() {
    let (id, Yaml::Map(fields)) =
        compose_job("build-macos-intel".to_owned(), job(&[])).expect("typed build composes")
    else {
        panic!("build is a map");
    };
    assert_eq!(id, "build-macos-intel");
    let env = fields
        .iter()
        .find(|(key, _)| key == "env")
        .map(|(_, value)| value)
        .expect("job environment");
    assert!(
        matches!(env, Yaml::Map(entries) if entries.contains(&("VELNOR_RELEASE_TARGET".to_owned(), Yaml::str("x86_64-apple-darwin"))))
    );
    let steps = fields
        .iter()
        .find(|(key, _)| key == "steps")
        .map(|(_, value)| value)
        .expect("steps");
    let Yaml::Seq(steps) = steps else {
        panic!("steps are a sequence");
    };
    assert!(matches!(
        steps.first(),
        Some(Yaml::Map(step)) if step.iter().any(|(key, value)| key == "with" && matches!(value, Yaml::Map(inputs) if inputs.contains(&("ref".to_owned(), Yaml::str("${{ inputs.source_sha }}")))))
    ));
}

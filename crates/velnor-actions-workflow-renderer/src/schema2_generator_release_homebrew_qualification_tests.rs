use super::*;
use crate::yaml::render_yaml;

#[test]
fn hosted_homebrew_job_is_read_only_bounded_and_scoped_to_exact_candidate_artifacts() {
    let pins = crate::schema2::product_release_test_pins::test_pins();
    let mut actions = Vec::new();
    let (id, job) = job(&pins, &mut actions).expect("render Homebrew qualification");
    let job_yaml = render_yaml(&job);
    let action_yaml = render_yaml(&actions[0].1);
    let Yaml::Map(job_fields) = &job else {
        panic!("qualification job is a mapping");
    };

    assert_eq!(id, "qualify-homebrew-macos");
    assert_eq!(
        actions[0].0,
        ".github/actions/generator-release-qualify-homebrew/action.yml"
    );
    assert!(job_yaml.contains("timeout-minutes: 30"));
    assert!(job_yaml.contains("contents: read"));
    assert!(job_yaml.contains("attestations: read"));
    assert!(!job_yaml.contains("contents: write"));
    assert!(!job_yaml.contains("id-token: write"));
    assert!(matches!(
        job_fields.iter().find(|(key, _)| key == "needs"),
        Some((_, Yaml::Seq(needs)))
            if needs == &[Yaml::str("build-macos"), Yaml::str("candidate-manifest")]
    ));

    assert!(action_yaml.contains("${{ inputs.artifact_id }}"));
    assert!(action_yaml.contains("${{ inputs.manifest_artifact_id }}"));
    assert!(action_yaml.contains("${{ inputs.manifest_sha256 }}"));
    let product = super::super::assets::MACOS_ARM64;
    let artifact_download = action_yaml
        .find("Download exact ARM candidate artifact")
        .expect("candidate artifact download step is rendered");
    let extraction = action_yaml
        .find("Prevalidate and extract uploaded candidate archive")
        .expect("archive extraction step is rendered");
    let manifest_download = action_yaml
        .find("Download canonical same-run candidate manifest")
        .expect("manifest download step is rendered");
    let provenance = action_yaml
        .find("Verify source-bound build record")
        .expect("provenance verification step is rendered");
    assert!(
        artifact_download < extraction
            && extraction < manifest_download
            && manifest_download < provenance,
        "download and extract candidate before provenance validation"
    );
    assert!(action_yaml.contains("scripts/with-owned-archive-guard.sh --"));
    assert!(action_yaml.contains(&format!(
        "extract-candidate '{}/{}' '{}' '{}' '{}' '{}'",
        product.directory,
        product.archive,
        product.directory,
        product.binary,
        product.sidecar,
        product.provenance
    )));
    assert!(action_yaml.contains("GH_TOKEN: ${{ github.token }}"));
    assert!(action_yaml.contains("VELNOR_INTERNAL_OP: qualify-homebrew-v1"));
    assert!(!action_yaml.contains("homebrew-qualify"));
    assert!(action_yaml.contains("velnor-actions-"));
}

#[test]
fn qualification_uses_pinned_rust_and_lint_tools_before_candidate_steps() {
    let pins = crate::schema2::product_release_test_pins::test_pins();
    let mut actions = Vec::new();
    job(&pins, &mut actions).expect("render Homebrew qualification");
    let action = &actions[0].1;
    let action_yaml = render_yaml(action);
    let candidate_download = action_yaml
        .find("Download exact ARM candidate artifact")
        .expect("candidate download is rendered");

    let expected_steps = [
        (
            "Install pinned qualification tools",
            super::workflow_steps::command_step(
                "Install pinned qualification tools",
                &pins.install_qualify_tools_argv,
            )
            .expect("render pinned qualification tool install"),
        ),
        (
            crate::VERIFY_RUST_TOOLCHAIN_STEP,
            super::workflow_steps::verify_rust_step(&pins).expect("render Rust verification"),
        ),
        (
            "Verify jactionlint",
            super::workflow_steps::command_step("Verify jactionlint", &pins.verify_lint_argv)
                .expect("render jactionlint verification"),
        ),
        (
            "Install pinned GitHub CLI",
            super::workflow_steps::install_gh_step(&pins.install_gh_argv)
                .expect("render pinned GitHub CLI install"),
        ),
    ];
    for (name, expected) in expected_steps {
        let step = named_step(action, name);
        assert_eq!(step, &expected, "step must use the ordinary pinned command");
        let Yaml::Map(fields) = step else {
            panic!("{name} is a step mapping");
        };
        assert_credentials_scrubbed(fields, name);
        let position = action_yaml
            .find(name)
            .unwrap_or_else(|| panic!("missing step {name}"));
        assert!(
            position < candidate_download,
            "{name} must precede candidate access"
        );
    }
    assert_eq!(
        action_yaml.matches("GH_TOKEN: ${{ github.token }}").count(),
        1
    );
}

fn assert_credentials_scrubbed(fields: &[(String, Yaml)], name: &str) {
    let Some(Yaml::Map(env)) = fields
        .iter()
        .find_map(|(key, value)| (key == "env").then_some(value))
    else {
        return;
    };
    for key in crate::toolchain_env::STEP_CREDENTIAL_DENYLIST {
        if let Some((_, value)) = env.iter().find(|(env_key, _)| env_key == key) {
            assert_eq!(value, &Yaml::str(""), "{name} must scrub {key}");
        }
    }
}

fn named_step<'a>(action: &'a Yaml, name: &str) -> &'a Yaml {
    let Yaml::Map(action_fields) = action else {
        panic!("composite action is a mapping");
    };
    let Yaml::Map(runs) = action_fields
        .iter()
        .find_map(|(key, value)| (key == "runs").then_some(value))
        .expect("runs field")
    else {
        panic!("runs is a mapping");
    };
    let Yaml::Seq(steps) = runs
        .iter()
        .find_map(|(key, value)| (key == "steps").then_some(value))
        .expect("steps field")
    else {
        panic!("steps is a sequence");
    };
    steps
        .iter()
        .find(|step| {
            let Yaml::Map(fields) = step else {
                return false;
            };
            fields
                .iter()
                .any(|(key, value)| key == "name" && value == &Yaml::str(name))
        })
        .unwrap_or_else(|| panic!("step {name} is rendered"))
}

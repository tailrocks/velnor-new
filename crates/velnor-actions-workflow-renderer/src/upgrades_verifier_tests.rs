use super::super::render_upgrades_workflow;
use super::{job_text, spec};

#[test]
fn upgrade_jactionlint_verifiers_use_the_canonical_release_renderer() {
    let spec = spec();
    let file = render_upgrades_workflow(&spec).expect("render");
    let yaml = crate::verifier_shell::render_command(
        "Verify jactionlint",
        &[
            "sh".to_owned(),
            "-c".to_owned(),
            spec.jactionlint_verify.clone(),
        ],
    )
    .expect("canonical release verifier");
    let expected = indent_workflow_step(&crate::yaml::render_yaml(&yaml));

    for id in ["prepare", "verify"] {
        assert!(
            job_text(&file.bytes, id).contains(&expected),
            "{id} must emit the exact typed, credential-scrubbed verifier step:\n{}",
            job_text(&file.bytes, id)
        );
    }
    assert!(expected.contains(&format!("shell: {}", crate::VERIFY_VERIFIER_SHELL)));
    assert!(expected.contains("unset ACTIONS_ID_TOKEN_REQUEST_TOKEN"));
    assert!(expected.contains("jactionlint_executable_digest_mismatch"));
    let executable_sha256 = velnor_actions_mise::jactionlint_executable_sha256(
        velnor_actions_mise::JACTIONLINT_VERSION,
        "linux-x64",
    )
    .expect("catalogued Jactionlint executable digest");
    assert!(
        expected.contains(&format!(
            "{}/linux-x64) want={executable_sha256};;",
            velnor_actions_mise::JACTIONLINT_VERSION
        )),
        "canonical executable digest row:\n{expected}"
    );
}

#[test]
fn upgrade_rust_verifier_preserves_the_credential_prefixed_script() {
    let mut upgrades = spec();
    upgrades.verify_rust_script = crate::toolchain_env::with_credential_unset_script(
        "v=\"${VELNOR_TOOL_RUST}\"; echo verify \"$v\"",
    );
    let prefix = crate::toolchain_env::credential_unset_prelude();
    let file = render_upgrades_workflow(&upgrades).expect("render");

    for id in ["prepare", "verify"] {
        let job = job_text(&file.bytes, id);
        let binding = job
            .lines()
            .find(|line| line.contains("VELNOR_VERIFY_RUST_SCRIPT:"))
            .expect("the public verifier script is bound into each generation job");
        assert!(
            binding.contains(&prefix),
            "{id} preserves the credential-unset prefix: {binding}"
        );
        assert!(
            job.contains("sh -c \\\"$VELNOR_VERIFY_RUST_SCRIPT\\\""),
            "{id} executes the bound script value"
        );
    }
}

fn indent_workflow_step(yaml: &str) -> String {
    let mut lines = yaml.lines();
    let first = lines.next().expect("step has a first line");
    let mut output = format!("      - {first}");
    for line in lines {
        output.push('\n');
        output.push_str("        ");
        output.push_str(line);
    }
    output
}

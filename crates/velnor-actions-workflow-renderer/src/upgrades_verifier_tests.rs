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

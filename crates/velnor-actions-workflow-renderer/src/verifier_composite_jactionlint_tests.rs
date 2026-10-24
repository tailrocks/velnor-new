use std::collections::BTreeMap;

use velnor_actions_contract::workflow::StepRole;

use super::{
    VERIFY_JACTIONLINT_USES, field, scalar, verifier_composite_document, verifier_shell_step,
};
use crate::{VERIFY_VERIFIER_SHELL, yaml::Yaml};

#[test]
fn jactionlint_composite_uses_the_trusted_shell_and_scrubbed_script() {
    let step = verifier_shell_step(
        StepRole::VerifyJactionlint,
        "Verify jactionlint",
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            crate::jactionlint_verifier_script("${VELNOR_TOOL_JACTIONLINT}"),
        ],
        BTreeMap::new(),
    )
    .expect("canonical Jactionlint verifier shell");
    let file = crate::verifier_composite::render_verifier_composite(
        VERIFY_JACTIONLINT_USES,
        &step,
        "0.1.40",
    )
    .expect("fixed Jactionlint composite");
    assert_eq!(
        file.path,
        ".github/actions/velnor-verify-jactionlint/action.yml"
    );
    assert!(file.bytes.contains("GITHUB_TOKEN: \"\""));
    assert!(
        file.bytes
            .contains("jactionlint_executable_digest_mismatch")
    );
    assert_eq!(
        file.bytes.matches("Resolve project tool versions").count(),
        1
    );

    let document = verifier_composite_document("Velnor Jactionlint verifier", &step)
        .expect("structured Jactionlint verifier composite");
    let Yaml::Seq(actions) = field(field(&document, "runs"), "steps") else {
        panic!("composite steps must be a sequence");
    };
    let verifier = actions.last().expect("verifier step follows resolver");
    assert_eq!(scalar(field(verifier, "shell")), VERIFY_VERIFIER_SHELL);
    let env = super::scalar_map(field(verifier, "env"));
    assert_eq!(
        env.get("PATH").map(String::as_str),
        Some(crate::VERIFY_PATH)
    );
    let run = scalar(field(verifier, "run"));
    assert!(
        run.starts_with("function_env_unset=(); function_env_scan_complete=0;"),
        "Jactionlint run keeps the function scan, outer env removals, fixed shell, and inner unset: {run}"
    );
    assert!(
        run.starts_with(crate::verifier_shell::tests::runtime::VERIFY_RUN_PREFIX),
        "Jactionlint run keeps exact function scan, outer env removals, fixed shell, and inner unset: {run}"
    );
    assert!(file.bytes.contains("VELNOR_TOOL_JACTIONLINT"));
}

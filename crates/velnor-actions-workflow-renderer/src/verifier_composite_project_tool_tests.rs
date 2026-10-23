use super::*;

#[test]
fn dynamic_verifier_composite_self_binds_its_project_tool_reference() {
    let step = rust_step(RUST_VERIFIER_USES[1]);
    let file = render_verifier_composite(RUST_VERIFIER_USES[1], &step, "0.1.40")
        .expect("dynamic verifier composite");
    let document = verifier_composite_document("Velnor Rust channel verifier", &step)
        .expect("structured dynamic verifier composite");
    let runs = field(&document, "runs");
    let Yaml::Seq(steps) = field(runs, "steps") else {
        panic!("composite steps must be a sequence");
    };
    assert_eq!(steps.len(), 2, "one resolver precedes the verifier");
    let resolver = &steps[0];
    assert_eq!(
        scalar(field(resolver, "name")),
        "Resolve project tool versions"
    );
    assert_eq!(scalar(field(resolver, "shell")), VERIFY_VERIFIER_SHELL);
    let resolver_run = scalar(field(resolver, "run"));
    assert!(resolver_run.contains("velnor_bind_selected_mise"));
    assert!(resolver_run.contains("function_env_scan_complete=0"));
    assert!(resolver_run.contains("unset ACTIONS_ID_TOKEN_REQUEST_TOKEN"));
    let verifier = &steps[1];
    assert_eq!(scalar(field(verifier, "shell")), VERIFY_VERIFIER_SHELL);
    assert!(scalar(field(verifier, "run")).contains("${VELNOR_TOOL_RUST}"));
    assert_eq!(
        file.bytes.matches("Resolve project tool versions").count(),
        1
    );

    let mut literal = step.clone();
    if let StepKind::Shell { run, .. } = &mut literal.kind {
        run[2] = run[2].replace("echo rust@${VELNOR_TOOL_RUST}", "echo rust@1.88.0");
    }
    let literal_document = verifier_composite_document("literal Rust verifier", &literal)
        .expect("literal verifier without a project reference");
    let literal_runs = field(&literal_document, "runs");
    let Yaml::Seq(literal_steps) = field(literal_runs, "steps") else {
        panic!("composite steps must be a sequence");
    };
    assert_eq!(
        literal_steps.len(),
        1,
        "no project reference needs no resolver"
    );

    let mut wrong_reference = rust_step(RUST_VERIFIER_USES[1]);
    if let StepKind::Shell { run, .. } = &mut wrong_reference.kind {
        run[2] = run[2].replace("VELNOR_TOOL_RUST", "VELNOR_TOOL_JACTIONLINT");
    }
    let error = render_verifier_composite(RUST_VERIFIER_USES[1], &wrong_reference, "0.1.40")
        .expect_err("the verifier cannot bind a different tool");
    assert!(
        error
            .to_string()
            .contains("verifier_composite_tool_binding"),
        "{error}"
    );
}

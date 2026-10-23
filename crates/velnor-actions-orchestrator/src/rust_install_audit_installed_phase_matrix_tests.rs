use super::{files, inline_step, problem, verifiers};

fn workflow_with_install(step: &str) -> String {
    format!(
        "jobs:\n  release:\n    steps:\n      - name: Install pinned Rust\n        run: mise install rust@1.99.1\n{step}"
    )
}

#[test]
fn every_installed_context_passes_and_the_restored_context_does_not() {
    let actions = verifiers();
    assert_eq!(actions.inline_installed_rust_templates.len(), 3);
    for (index, template) in actions.inline_installed_rust_templates.iter().enumerate() {
        let step = inline_step("Neutral installed verifier", template);
        let found = problem(&files(
            ".github/workflows/release.yml",
            &workflow_with_install(&step),
        ));
        assert!(
            found.is_empty(),
            "installed context {index} must pass: {found}"
        );
    }

    let restored = &actions.inline_rust_templates["Verify restored Rust toolchain"];
    assert_eq!(restored.len(), 1);
    let step = inline_step("Neutral restored verifier", &restored[0]);
    let found = problem(&files(
        ".github/workflows/release.yml",
        &workflow_with_install(&step),
    ));
    assert!(
        found.contains("no verification follows the install"),
        "a restored-cache verifier cannot satisfy the installed phase: {found}"
    );
}

use super::{files, problem};

const FRESHNESS: &str = include_str!("../../../.github/workflows/freshness.yml");

fn project_tool_resolver_step() -> String {
    super::super::parse::step_lists(FRESHNESS)
        .into_iter()
        .flat_map(|(_, steps)| steps)
        .find(|step| step.name == "Resolve project tool versions")
        .expect("generated freshness project-tool resolver")
        .text
}

#[test]
fn project_tool_resolver_shell_is_not_a_rust_verifier() {
    let resolver = project_tool_resolver_step();
    let only_resolver = format!("jobs:\n  resolver:\n    steps:\n{resolver}");
    assert_eq!(
        problem(&files(".github/workflows/freshness.yml", &only_resolver)),
        "",
        "the canonical project-tool resolver may share the protected shell"
    );

    let after_install = format!(
        "jobs:\n  build:\n    steps:\n      - name: Install pinned Rust\n        run: mise install rust@1.99.1\n{resolver}"
    );
    let found = problem(&files(".github/workflows/freshness.yml", &after_install));
    assert!(
        found.contains("no verification follows the install"),
        "the project-tool resolver must not satisfy installed-Rust verification: {found}"
    );
}

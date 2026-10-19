//! Production release pins must preserve the ordinary validator's narrow bridge.
use super::*;

#[test]
fn production_release_actionlint_uses_exact_pinned_tools_and_authoritative_ignores() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let config = crate::config::load_config(&root).expect("repository release configuration");
    let pins = resolve(&config).expect("production release pins");
    let catalog = ToolCatalog::pinned();
    let mut expected = exec_argv(
        &[PinnedTool::Actionlint, PinnedTool::Shellcheck],
        "actionlint",
        &["-color"],
        &catalog,
    )
    .expect("pinned actionlint and ShellCheck argv");
    let ignores = velnor_actions_actionlint::step_parallelism_ignore_args();
    assert!(
        !ignores.is_empty(),
        "pinned parser requires the native syntax bridge"
    );
    expected.extend(ignores);
    assert_eq!(pins.actionlint_argv, expected);
    assert_eq!(
        pins.actionlint_argv
            .iter()
            .filter(|a| *a == "-ignore")
            .count(),
        2
    );
    assert!(!pins.actionlint_argv.iter().any(|a| a == "-shellcheck="));
}

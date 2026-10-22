//! Adapter wiring: the owned config symbol and sibling tool files.
use velnor_actions_actionlint::{
    FOREIGN_TOOL_FILES, JACTIONLINT_CONFIG_FILE, JACTIONLINT_CONFIG_PATH, OWNED_SYMBOLS, TOOL_ID,
    is_owned_jactionlint_file, stack_for_symbol,
};

#[test]
fn the_adapter_owns_only_the_jactionlint_config() {
    assert_eq!(TOOL_ID, "jactionlint");
    assert_eq!(JACTIONLINT_CONFIG_FILE, "jactionlint.yaml");
    assert_eq!(
        JACTIONLINT_CONFIG_PATH,
        format!(".github/{JACTIONLINT_CONFIG_FILE}")
    );
    assert_eq!(OWNED_SYMBOLS, &["jactionlint.yaml"]);
    assert_eq!(
        stack_for_symbol(".github/jactionlint.yaml"),
        Some("jactionlint")
    );
    assert!(is_owned_jactionlint_file(".github/jactionlint.yaml"));
    for retired in [
        "actionlint.yaml",
        ".github/actionlint.yaml",
        ".actionlint.yaml",
    ] {
        assert!(!is_owned_jactionlint_file(retired), "{retired}");
        assert_eq!(stack_for_symbol(retired), None, "{retired}");
    }
    for symbol in FOREIGN_TOOL_FILES {
        let owner = stack_for_symbol(symbol);
        assert!(
            matches!(owner, Some("mise" | "rust")),
            "{symbol}: {owner:?}"
        );
    }
}

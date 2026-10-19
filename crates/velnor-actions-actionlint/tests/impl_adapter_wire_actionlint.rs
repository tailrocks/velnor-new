//! Adapter-wire cases: actionlint metadata routing (RQ-1.4).
use velnor_actions_actionlint::{
    ACTIONLINT_CONFIG_FILE, FOREIGN_TOOL_FILES, OWNED_SYMBOLS, is_owned_actionlint_file,
    stack_for_symbol,
};

#[test]
fn actionlint_symbols_route_to_owning_stacks() {
    assert_eq!(ACTIONLINT_CONFIG_FILE, "actionlint.yaml");
    for symbol in OWNED_SYMBOLS {
        assert_eq!(stack_for_symbol(symbol), Some("actionlint"), "{symbol}");
        assert!(is_owned_actionlint_file(symbol));
    }
    assert_eq!(
        stack_for_symbol(".github/actionlint.yaml"),
        Some("actionlint")
    );
    for symbol in FOREIGN_TOOL_FILES {
        let owner = stack_for_symbol(symbol);
        assert!(owner == Some("mise") || owner == Some("rust"), "{symbol}");
        assert!(!is_owned_actionlint_file(symbol));
    }
    assert_eq!(stack_for_symbol("mise.toml"), Some("mise"));
    assert_eq!(stack_for_symbol("Cargo.toml"), Some("rust"));
    assert_eq!(stack_for_symbol("README.md"), None);
    assert!(!is_owned_actionlint_file("README.md"));
}

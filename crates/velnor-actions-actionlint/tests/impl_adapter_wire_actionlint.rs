//! Adapter-wire cases: actionlint metadata routing and per-concern
//! native-parallelism qualification (RQ-1.4, PAR-6.4 halves).
use velnor_actions_actionlint::{
    ACTIONLINT_CONFIG_FILE, ActionlintCapabilities, FOREIGN_TOOL_FILES, NativeParallelismConcerns,
    OWNED_SYMBOLS, StepSyntax, is_owned_actionlint_file, stack_for_symbol,
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

#[test]
fn native_parallelism_needs_every_concern() {
    let pinned = ActionlintCapabilities::for_pinned();
    assert!(!NativeParallelismConcerns::none().qualified());
    assert!(NativeParallelismConcerns::all().qualified());
    let partial = NativeParallelismConcerns {
        timing: false,
        ..NativeParallelismConcerns::all()
    };
    assert!(!partial.qualified());
    let still_closed = pinned.qualify_native_step_parallelism_if(partial);
    assert!(!still_closed.native_step_parallelism_qualified());
    assert!(
        still_closed
            .check_step_syntax(StepSyntax::NativeParallelism)
            .is_err()
    );
    let qualified = pinned.qualify_native_step_parallelism_if(NativeParallelismConcerns::all());
    assert!(qualified.native_step_parallelism_qualified());
    assert_eq!(
        qualified.check_step_syntax(StepSyntax::NativeParallelism),
        Ok(())
    );
    assert_eq!(pinned.check_step_syntax(StepSyntax::JobMatrix), Ok(()));
}

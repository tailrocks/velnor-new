use super::*;

#[test]
fn every_rust_kind_has_one_fixed_phase() {
    for (kind, expected) in [
        ("fmt", RustJobPhase::Lint),
        ("clippy", RustJobPhase::Lint),
        ("build", RustJobPhase::Tests),
        ("test", RustJobPhase::Tests),
        ("nextest", RustJobPhase::Tests),
        ("doctest", RustJobPhase::Tests),
        ("doc", RustJobPhase::Docs),
    ] {
        assert_eq!(
            RustJobPhase::from_task_kind(kind).expect("known kind"),
            expected
        );
    }
    assert_eq!(RustJobPhase::Lint.as_str(), "lint");
    assert_eq!(RustJobPhase::Tests.as_str(), "tests");
    assert_eq!(RustJobPhase::Docs.as_str(), "docs");
    assert_eq!(RustJobPhase::Docs.display_name(), "Documentation");
}

#[test]
fn unknown_rust_kind_cannot_acquire_an_owner() {
    for kind in ["", "Doc", "archive", "lint", "doc "] {
        assert!(RustJobPhase::from_task_kind(kind).is_err(), "{kind:?}");
    }
}

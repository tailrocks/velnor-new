use super::super::normalize_identity_path;

#[test]
fn identity_paths_preserve_case_and_reject_malformed() {
    assert_eq!(
        normalize_identity_path("Crates/Äpfel/Cargo.toml").expect("unicode"),
        "Crates/Äpfel/Cargo.toml"
    );
    for bad in ["", "/abs/path", "a/../b", "a\\b", "a\0b", "a\nb"] {
        assert!(normalize_identity_path(bad).is_err(), "{bad:?}");
    }
}

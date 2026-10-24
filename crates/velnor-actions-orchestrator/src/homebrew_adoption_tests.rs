use super::*;

#[test]
fn protected_transition_selection_accepts_only_admitted_repository_and_base() {
    let admitted = protected_homebrew_adoption_transition(
        "jackin-project/homebrew-tap",
        1_200_197_694,
        "921858f118256e16d3dc312c407814ab0fd8ef52",
    )
    .expect("protected transition");
    assert_eq!(
        admitted.transition().base_tree,
        "ce52780173323bcfcecce152cc147d342d17b6cc"
    );
    assert!(
        protected_homebrew_adoption_transition(
            "jackin-project/homebrew-tap",
            1_200_197_694,
            &"a".repeat(40),
        )
        .is_err()
    );
    assert!(
        protected_homebrew_adoption_transition(
            "attacker/homebrew-tap",
            1_200_197_694,
            "921858f118256e16d3dc312c407814ab0fd8ef52",
        )
        .is_err()
    );
    assert!(
        protected_homebrew_adoption_transition(
            "jackin-project/homebrew-tap",
            7,
            "921858f118256e16d3dc312c407814ab0fd8ef52",
        )
        .is_err()
    );
}

#[test]
fn parses_only_complete_nul_framed_raw_diff_records() {
    let old = "a".repeat(40);
    let new = "b".repeat(40);
    let raw = format!(":100644 100644 {old} {new} M\0mise.toml\0");
    let changes = parse_raw_diff(raw.as_bytes()).expect("raw diff");
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].path, "mise.toml");
    assert_eq!(changes[0].status, "M");
    assert!(parse_raw_diff(raw.trim_end_matches('\0').as_bytes()).is_err());
    assert!(parse_raw_diff(b":100644 100644 bad bad M\0mise.toml\0").is_err());
    assert!(parse_raw_diff(b":100644 100644 aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb M\0../mise.toml\0").is_err());
}

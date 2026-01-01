use super::*;

#[test]
fn identity_problems_render_as_one_truncated_line() {
    let forged = "evil\n::notice::spoofed\r\nnext";
    let err = ContractError::identity("conclusion", format!("unknown_conclusion:{forged}"));
    let text = err.to_string();
    assert!(!text.contains('\n') && !text.contains('\r'), "{text}");
    assert!(
        text.contains("unknown_conclusion:evil::notice::spoofednext"),
        "{text}"
    );
    let err = ContractError::identity("field", "clean_code");
    assert_eq!(err.to_string(), "invalid field: clean_code");
}

#[test]
fn long_problems_truncate_at_120_chars() {
    let long = format!("prefix:{}", "v".repeat(200));
    let err = ContractError::identity("field", long);
    let text = err.to_string();
    let problem = text.strip_prefix("invalid field: ").expect("prefix");
    assert_eq!(problem.chars().count(), 120, "{text}");
    assert!(problem.starts_with("prefix:vv"), "{text}");
}

#[test]
fn config_problems_sanitize_the_same_way() {
    let err = ContractError::config("f.toml", "k", "bad_target:x\n forged");
    let text = err.to_string();
    assert!(!text.contains('\n'), "{text}");
    assert!(text.contains("bad_target:x forged"), "{text}");
}

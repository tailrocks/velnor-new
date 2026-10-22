use super::validate_completion_app_ids;

#[test]
fn completion_app_must_be_separate_from_writer_and_github_actions() {
    assert!(validate_completion_app_ids(17, 19).is_ok());
    assert!(validate_completion_app_ids(17, 17).is_err());
    assert!(validate_completion_app_ids(17, 15_368).is_err());
}

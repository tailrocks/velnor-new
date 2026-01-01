#![cfg(unix)]
use super::GitRequest;
use crate::CheckDeadline;
use std::time::Duration;

#[test]
fn expired_shared_deadline_rejects_git_before_spawn() {
    let deadline = CheckDeadline::after(Duration::ZERO).expect("deadline");
    let request = GitRequest::rev_parse(vec!["HEAD".into()]);
    let error = request
        .run_in_until(std::path::Path::new("."), deadline)
        .expect_err("expired git request");
    assert!(
        error
            .to_string()
            .contains("timeout_after_absolute_deadline")
    );
}

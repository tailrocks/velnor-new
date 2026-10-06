use super::verify_checkout_until;
use std::time::Duration;
use velnor_actions_contract::WorkflowEvent;
use velnor_actions_mise::CheckDeadline;

#[test]
fn checkout_git_must_fit_the_shared_deadline() {
    let root = tempfile::tempdir().expect("checkout root");
    let started = std::time::Instant::now()
        .checked_sub(Duration::from_millis(20))
        .expect("past start");
    let deadline = CheckDeadline::from_start(started, Duration::from_millis(10)).expect("deadline");
    let error = verify_checkout_until(
        root.path(),
        WorkflowEvent::Push,
        "0123456789abcdef0123456789abcdef01234567",
        deadline,
    )
    .expect_err("expired Git checkout verification");
    assert!(
        error
            .to_string()
            .contains("timeout_after_absolute_deadline"),
        "checkout reports its expired shared deadline: {error}"
    );
}

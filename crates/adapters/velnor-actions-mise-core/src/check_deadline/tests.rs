use super::CheckDeadline;
use crate::IsolatedCommand;
use std::ffi::OsString;
use std::time::Duration;

#[test]
fn one_deadline_limits_a_later_probe_after_preparation_time_is_used() {
    let deadline = CheckDeadline::after(Duration::from_millis(150)).expect("deadline");
    std::thread::sleep(Duration::from_millis(80));
    let command = IsolatedCommand::qualified_check_probe(
        OsString::from("/bin/sleep"),
        vec![OsString::from("1")],
        Vec::new(),
    );
    let result = command.run_until(1024, deadline);
    assert!(result.is_err());
    assert!(deadline.remaining().is_err());
}

#[test]
fn deadline_from_operation_start_includes_elapsed_setup_time() {
    let started = std::time::Instant::now()
        .checked_sub(Duration::from_millis(20))
        .expect("past instant");
    let deadline = CheckDeadline::from_start(started, Duration::from_millis(10)).expect("deadline");
    assert!(deadline.remaining().is_err());
}

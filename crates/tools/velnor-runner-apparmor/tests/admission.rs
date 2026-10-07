//! Admission boundary: profile verification fails closed until a
//! compiled policy identity is approved in the binary.

use velnor_runner_apparmor::verify_runner_profile;
use velnor_runner_journal::HostError;

#[test]
fn unapproved_policy_identity_fails_closed() {
    assert_eq!(verify_runner_profile(), Err(HostError::Config));
}

#[test]
fn admission_failure_is_deterministic_and_secret_free() -> Result<(), String> {
    let Err(first) = verify_runner_profile() else {
        return Err("admission succeeded without an approved policy".to_owned());
    };
    let Err(second) = verify_runner_profile() else {
        return Err("admission succeeded without an approved policy".to_owned());
    };
    assert_eq!(first, second);
    assert_eq!(format!("{first}"), "invalid config");
    Ok(())
}

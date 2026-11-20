// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::{ErrorDetail, HostError};
use crate::scale_set::EnsureError;

fn shown(raw: &[u8]) -> String {
    ErrorDetail::from_bytes(raw).to_string()
}

#[test]
fn line_breaks_become_one_space_between_words() {
    assert_eq!(shown(b"first\r\nsecond\nthird"), "first second third");
}

#[test]
fn color_escapes_leave_no_remains() {
    assert_eq!(shown(b"\x1b[31;1mred\x1b[0m done"), "red done");
}

#[test]
fn a_poll_failure_keeps_its_own_message() {
    let error = HostError::poll(&EnsureError::Rejected);
    assert_eq!(error.to_string(), "poll failed: registration rejected");
}

#[test]
fn a_host_error_stays_small_enough_to_return_by_value() {
    assert!(std::mem::size_of::<HostError>() <= 72);
}

#[test]
fn the_probe_reason_fits_whole() {
    let message = b"resource probe: helper_source_revision_missing_invalid";
    assert_eq!(shown(message).as_bytes(), message);
}

#[test]
fn a_detail_longer_than_the_limit_keeps_its_first_64_printable_bytes() {
    let mut raw = vec![0x07_u8];
    raw.extend((b'a'..=b'z').cycle().take(100));
    let expected: String = (b'a'..=b'z').cycle().take(64).map(char::from).collect();
    assert_eq!(shown(&raw), expected);
}

#[test]
fn a_double_401_is_unauthorized_and_transport_uncertainty_stays_uncertain() {
    use velnor_runner_github::{SessionError, WireError};
    assert_eq!(
        crate::listen::map_listen(SessionError::Wire(WireError::RefreshExhausted)),
        EnsureError::Unauthorized
    );
    assert_eq!(
        crate::listen::map_listen(SessionError::Uncertain),
        EnsureError::Uncertain
    );
}

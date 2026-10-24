//! Tests for the worker input and acknowledgment path.

use super::{AckRead, wait_for_ack_with};
use std::{cell::Cell, collections::VecDeque, time::Duration};
use velnor_resource_probe::{ProbeError, worker_binding::WORKER_ACK_LIMIT};

const OPERATION: &str = "0123456789abcdef0123456789abcdef";

fn ack() -> Vec<u8> {
    format!("{{\"operation_id\":\"{OPERATION}\"}}\n").into_bytes()
}

#[test]
fn complete_line_releases_helper_while_input_stays_open() {
    let line = ack();
    let mut reads = 0;
    let result = wait_for_ack_with(
        OPERATION,
        Duration::from_secs(30),
        || Duration::ZERO,
        |_, output| {
            reads += 1;
            if reads == 1 {
                output[..line.len()].copy_from_slice(&line);
                Ok(AckRead::Data(line.len()))
            } else {
                Ok(AckRead::Timeout)
            }
        },
    );
    assert!(result.is_ok());
    assert_eq!(reads, 2);
}

#[test]
fn fragmented_line_uses_one_deadline() {
    let line = ack();
    let split = line.len() / 2;
    let clock = Cell::new(Duration::ZERO);
    let mut chunks = VecDeque::from([line[..split].to_vec(), line[split..].to_vec()]);
    let mut remaining_seen = Vec::new();
    let result = wait_for_ack_with(
        OPERATION,
        Duration::from_secs(30),
        || clock.get(),
        |remaining, output| {
            remaining_seen.push(remaining);
            let Some(chunk) = chunks.pop_front() else {
                return Ok(AckRead::Eof);
            };
            output[..chunk.len()].copy_from_slice(&chunk);
            if remaining_seen.len() == 1 {
                clock.set(Duration::from_secs(8));
            }
            Ok(AckRead::Data(chunk.len()))
        },
    );
    assert!(result.is_ok());
    assert_eq!(
        remaining_seen,
        [
            Duration::from_secs(30),
            Duration::from_secs(22),
            Duration::ZERO
        ]
    );
}

#[test]
fn malformed_mismatched_and_extra_frames_fail() {
    let wrong = format!("{{\"operation_id\":\"{}\"}}\n", "f".repeat(32));
    for bytes in [wrong.into_bytes(), b"not-json\n".to_vec()] {
        assert!(read_one(&bytes).is_err());
    }
    let mut extra = ack();
    extra.extend_from_slice(b"{}\n");
    assert!(read_one(&extra).is_err());
    let line = ack();
    let mut chunks = VecDeque::from([line, b"{}\n".to_vec()]);
    assert!(
        wait_for_ack_with(
            OPERATION,
            Duration::from_secs(30),
            || Duration::ZERO,
            |_, output| {
                let Some(chunk) = chunks.pop_front() else {
                    return Ok(AckRead::Timeout);
                };
                output[..chunk.len()].copy_from_slice(&chunk);
                Ok(AckRead::Data(chunk.len()))
            },
        )
        .is_err()
    );
}

fn read_one(bytes: &[u8]) -> Result<(), ProbeError> {
    wait_for_ack_with(
        OPERATION,
        Duration::from_secs(30),
        || Duration::ZERO,
        |_, output| {
            let count = bytes.len().min(output.len());
            output[..count].copy_from_slice(&bytes[..count]);
            Ok(AckRead::Data(count))
        },
    )
}

#[test]
fn eof_timeout_and_size_bounds_fail_without_waiting() {
    assert!(
        wait_for_ack_with(
            OPERATION,
            Duration::from_secs(30),
            || Duration::ZERO,
            |_, _| Ok(AckRead::Eof),
        )
        .is_err()
    );
    assert!(
        wait_for_ack_with(
            OPERATION,
            Duration::from_secs(30),
            || Duration::ZERO,
            |_, _| Ok(AckRead::Timeout),
        )
        .is_err()
    );
    let oversized = vec![b'x'; WORKER_ACK_LIMIT + 1];
    assert!(read_one(&oversized).is_err());
    let clock = Cell::new(Duration::ZERO);
    assert!(
        wait_for_ack_with(
            OPERATION,
            Duration::from_secs(30),
            || clock.get(),
            |_, output| {
                let line = ack();
                output[..line.len()].copy_from_slice(&line);
                clock.set(Duration::from_secs(30));
                Ok(AckRead::Data(line.len()))
            },
        )
        .is_err()
    );
    let calls = Cell::new(0);
    let mut called = false;
    assert!(
        wait_for_ack_with(
            OPERATION,
            Duration::from_secs(30),
            || {
                let call = calls.get();
                calls.set(call + 1);
                if call == 0 {
                    Duration::ZERO
                } else {
                    Duration::from_secs(30)
                }
            },
            |_, _| {
                called = true;
                Ok(AckRead::Timeout)
            },
        )
        .is_err()
    );
    assert!(!called);
}

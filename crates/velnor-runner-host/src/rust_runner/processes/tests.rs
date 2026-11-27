// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{MachineProcess, parse_ps};
use crate::error::RustRunnerFailure;

#[test]
fn the_ps_parse_accepts_padded_pids_and_crlf_and_fails_closed() {
    assert_eq!(parse_ps(""), Ok(Vec::new()));
    assert_eq!(
        parse_ps("  101 /bin/sh -c x\r\n7\r\n 4242   sleep 600\n"),
        Ok(vec![
            MachineProcess {
                pid: 101,
                args: "/bin/sh -c x".to_owned()
            },
            MachineProcess {
                pid: 7,
                args: String::new()
            },
            MachineProcess {
                pid: 4242,
                args: "sleep 600".to_owned()
            },
        ])
    );
    for malformed in ["abc sleep", "101 ok\nPID COMMAND\n", "-5 x", "12x y"] {
        assert_eq!(
            parse_ps(malformed),
            Err(RustRunnerFailure::ProcessList),
            "{malformed}"
        );
    }
}

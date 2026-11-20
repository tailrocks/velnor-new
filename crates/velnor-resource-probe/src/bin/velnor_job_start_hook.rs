// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![allow(
    missing_docs,
    reason = "documentation comments are intentionally absent from this codebase"
)]
#![allow(
    clippy::missing_errors_doc,
    reason = "documentation comments are intentionally absent from this codebase"
)]
use std::io::Write;

fn main() {
    if let Err(error) = velnor_resource_probe::job_hook::run() {
        let message = error.to_string();
        let _write_result = writeln!(std::io::stderr().lock(), "job hook: {message}");
        let _kill_result = rustix::process::kill_process_group(
            rustix::process::getpgrp(),
            rustix::process::Signal::KILL,
        );
        loop {
            std::thread::park();
        }
    }
}

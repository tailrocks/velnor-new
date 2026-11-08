// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};
use velnor_actions_mise::CheckDeadline;

fn main() -> ExitCode {
    let args = std::env::args_os().collect::<Vec<_>>();
    let Some(path) = args
        .windows(2)
        .find(|pair| pair[0] == "--archive-guard")
        .map(|pair| PathBuf::from(&pair[1]))
    else {
        return ExitCode::FAILURE;
    };
    let Ok(initial) = std::fs::metadata(&path) else {
        return ExitCode::FAILURE;
    };
    let identity = (initial.dev(), initial.ino());
    let code = CheckDeadline::with_clock(
        move || {
            let changed = std::fs::metadata(&path)
                .map_or(true, |current| (current.dev(), current.ino()) != identity);
            if changed {
                Instant::now() + Duration::from_secs(3_601)
            } else {
                Instant::now()
            }
        },
        velnor_archive_guard::golden_collector::run_cli,
    );
    ExitCode::from(code)
}

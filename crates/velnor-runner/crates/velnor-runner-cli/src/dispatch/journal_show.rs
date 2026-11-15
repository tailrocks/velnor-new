// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::ExitCode;

pub(super) fn show(state: &Path, reservation: Option<i64>) -> ExitCode {
    match velnor_runner_host::show_reservations(state, reservation) {
        Ok(rows) => {
            for row in rows {
                println!("{}", row.render());
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("journal show failed ({}): {error}", error.code());
            ExitCode::from(1)
        }
    }
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io;
use std::path::Path;

use tokio::fs;

pub(super) async fn recorded_pid(path: &Path) -> io::Result<Option<String>> {
    let text = match fs::read_to_string(path).await {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return match fs::symlink_metadata(path).await {
                Ok(_) => Err(io::Error::other(format!(
                    "{} is a dangling link, not a process id; check whether the process is still running, then remove the link and run this again",
                    path.display()
                ))),
                Err(_) => Ok(None),
            };
        }
        Err(error) => {
            return Err(io::Error::new(
                error.kind(),
                format!("cannot read {}: {error}", path.display()),
            ));
        }
    };
    let pid = text.trim();
    if pid.parse::<u32>().is_err() {
        return Err(io::Error::other(format!(
            "{} does not hold a process id; check whether the process is still running, then remove the file and run this again",
            path.display()
        )));
    }
    Ok(Some(pid.to_owned()))
}

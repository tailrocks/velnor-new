// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use crate::error::CiError;

use super::crates_mirror::{CONFIG_ENV, DISABLE_ENV, failed};

const FILE: &str = "velnor-cargo-mode";

#[derive(Clone, Copy)]
pub(super) struct Mode {
    pub(super) mirror: bool,
    pub(super) mbx: bool,
}

pub(super) fn write(directory: &Path, mode: Mode) -> Result<(), CiError> {
    let text = format!(
        "mirror={}\nmbx={}\n",
        if mode.mirror { "on" } else { "off" },
        if mode.mbx { "required" } else { "none" }
    );
    let staged = directory.join(format!("{FILE}.tmp"));
    std::fs::write(&staged, text)
        .and_then(|()| std::fs::rename(&staged, directory.join(FILE)))
        .map_err(|error| failed(&error))
}

pub(super) fn recorded() -> Result<Option<Mode>, CiError> {
    let Some(executable) = std::env::current_exe()
        .ok()
        .and_then(|executable| std::fs::canonicalize(executable).ok())
    else {
        return Ok(None);
    };
    let Some(directory) = executable.parent() else {
        return Ok(None);
    };
    let candidates: Vec<PathBuf> = [Some(directory), directory.parent()]
        .into_iter()
        .flatten()
        .map(|directory| directory.join(FILE))
        .collect();
    for path in candidates {
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                return parse(&text).map(Some).map_err(|reason| {
                    CiError::new(format!(
                        "{} is not a valid mode record: {reason}",
                        path.display()
                    ))
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(failed(&error)),
        }
    }
    Ok(None)
}

fn parse(text: &str) -> Result<Mode, &'static str> {
    let mut lines = text.lines();
    let mirror = match lines.next() {
        Some("mirror=on") => true,
        Some("mirror=off") => false,
        _ => return Err("expected mirror=on or mirror=off"),
    };
    let mbx = match lines.next() {
        Some("mbx=required") => true,
        Some("mbx=none") => false,
        _ => return Err("expected mbx=required or mbx=none"),
    };
    Ok(Mode { mirror, mbx })
}

pub(super) fn mirror_off() -> Result<bool, CiError> {
    let live_off = std::env::var(DISABLE_ENV).is_ok_and(|value| value == "off");
    match recorded()? {
        Some(recorded) if recorded.mirror && live_off => Err(CiError::new(format!(
            "{DISABLE_ENV}=off is set in a job whose mirror setup chose the mirror; rerun the crates-mirror step with {DISABLE_ENV}=off instead"
        ))),
        Some(recorded) => Ok(!recorded.mirror),
        None => Ok(live_off),
    }
}

pub(super) fn require_config(
    mirror_off: bool,
    config: Option<std::ffi::OsString>,
) -> Result<Option<std::ffi::OsString>, CiError> {
    let config = config.filter(|config| !config.is_empty());
    if config.is_none() && !mirror_off {
        return Err(CiError::new(format!(
            "missing {CONFIG_ENV} (unset or empty)"
        )));
    }
    Ok(config)
}

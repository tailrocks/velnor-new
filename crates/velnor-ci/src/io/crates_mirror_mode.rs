// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use crate::error::CiError;

use super::crates_mirror::{CONFIG_ENV, DISABLE_ENV, failed};
use super::crates_mirror_mbx::ENV as MBX_ENV;

const FILE: &str = "velnor-cargo-mode";

#[derive(Clone, PartialEq, Eq, Debug)]
pub(super) enum Mirror {
    Off,
    On(PathBuf),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub(super) struct Mode {
    pub(super) mirror: Mirror,
    pub(super) mbx: bool,
}

pub(super) fn write(directory: &Path, mode: &Mode) -> Result<(), CiError> {
    let config = match &mode.mirror {
        Mirror::On(config) if config.to_string_lossy().contains('\n') => {
            return Err(CiError::new("the mirror config path contains a newline"));
        }
        Mirror::On(config) => format!("config={}\n", config.to_string_lossy()),
        Mirror::Off => String::new(),
    };
    let text = format!(
        "mirror={}\nmbx={}\n{config}",
        if mode.mirror == Mirror::Off {
            "off"
        } else {
            "on"
        },
        if mode.mbx { "required" } else { "none" }
    );
    let staged = directory.join(format!("{FILE}.tmp"));
    match std::fs::symlink_metadata(&staged) {
        Ok(metadata) if metadata.is_dir() => {
            return Err(CiError::new(format!(
                "{} is a directory, not a stale record",
                staged.display()
            )));
        }
        Ok(_) => std::fs::remove_file(&staged).map_err(|error| failed(&error))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(failed(&error)),
    }
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .open(&staged)
        .and_then(|mut file| file.write_all(text.as_bytes()))
        .and_then(|()| std::fs::rename(&staged, directory.join(FILE)))
        .map_err(|error| failed(&error))
}

fn env_requires_the_mirror() -> bool {
    let mbx = std::env::var_os(MBX_ENV).is_some_and(|value| !value.is_empty());
    let mirror = std::env::var_os(CONFIG_ENV).is_some_and(|value| !value.is_empty())
        && !std::env::var(DISABLE_ENV).is_ok_and(|value| value == "off");
    mbx || mirror
}

pub(super) fn recorded() -> Result<Option<Mode>, CiError> {
    let executable = std::env::current_exe()
        .ok()
        .and_then(|executable| std::fs::canonicalize(executable).ok());
    recorded_for(executable.as_deref(), env_requires_the_mirror())
}

fn recorded_for(
    executable: Option<&Path>,
    env_requires_the_mirror: bool,
) -> Result<Option<Mode>, CiError> {
    let Some(directory) = executable.and_then(Path::parent) else {
        if env_requires_the_mirror {
            return Err(CiError::new(
                "cannot locate the cargo wrapper's mode record, and the job env requires the crates mirror or mbx",
            ));
        }
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
    let mirror_on = match lines.next() {
        Some("mirror=on") => true,
        Some("mirror=off") => false,
        _ => return Err("expected mirror=on or mirror=off"),
    };
    let mbx = match lines.next() {
        Some("mbx=required") => true,
        Some("mbx=none") => false,
        _ => return Err("expected mbx=required or mbx=none"),
    };
    let mirror = if mirror_on {
        match lines.next().and_then(|line| line.strip_prefix("config=")) {
            Some(config) if Path::new(config).is_absolute() => Mirror::On(PathBuf::from(config)),
            _ => return Err("expected an absolute config=<path> after mirror=on"),
        }
    } else {
        Mirror::Off
    };
    Ok(Mode { mirror, mbx })
}

pub(super) fn mirror_off() -> Result<bool, CiError> {
    let live_off = std::env::var(DISABLE_ENV).is_ok_and(|value| value == "off");
    match recorded()? {
        Some(Mode {
            mirror: Mirror::On(_),
            ..
        }) if live_off => Err(CiError::new(format!(
            "{DISABLE_ENV}=off is set in a job whose mirror setup chose the mirror; rerun the crates-mirror step with {DISABLE_ENV}=off instead"
        ))),
        Some(recorded) => Ok(recorded.mirror == Mirror::Off),
        None => Ok(live_off),
    }
}

pub(super) fn require_config(
    mirror_off: bool,
    config: Option<OsString>,
) -> Result<Option<OsString>, CiError> {
    let config = config.filter(|config| !config.is_empty());
    if let Some(Mode {
        mirror: Mirror::On(recorded),
        ..
    }) = recorded()?
    {
        return match config {
            Some(config) if names_the_same_file(&config, &recorded) => {
                Ok(Some(recorded.into_os_string()))
            }
            Some(config) => Err(CiError::new(format!(
                "{CONFIG_ENV} is {}, but the crates-mirror setup recorded {}",
                Path::new(&config).display(),
                recorded.display()
            ))),
            None => Err(CiError::new(format!(
                "missing {CONFIG_ENV} (unset or empty)"
            ))),
        };
    }
    if config.is_none() && !mirror_off {
        return Err(CiError::new(format!(
            "missing {CONFIG_ENV} (unset or empty)"
        )));
    }
    Ok(config)
}

fn names_the_same_file(config: &OsString, recorded: &Path) -> bool {
    config == recorded.as_os_str()
        || matches!(
            (std::fs::canonicalize(config), std::fs::canonicalize(recorded)),
            (Ok(config), Ok(recorded)) if config == recorded
        )
}

#[cfg(test)]
mod tests {
    use super::recorded_for;

    #[test]
    fn an_unlocatable_wrapper_fails_closed_when_the_job_env_requires_the_mirror() {
        assert!(recorded_for(None, true).is_err());
    }

    #[test]
    fn an_unlocatable_wrapper_falls_back_when_nothing_requires_the_mirror() {
        assert!(matches!(recorded_for(None, false), Ok(None)));
    }
}

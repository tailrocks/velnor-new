// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use super::{Error, Exit, read_tracked, toplevel};
use crate::{release_ruleset, release_ruleset_drift};

pub(super) fn print(root: &Path) -> Exit {
    match ruleset(root) {
        Ok(json) => {
            print!("{json}");
            Exit::SUCCESS
        }
        Err(error) => {
            eprintln!("velnor-gen: {error}");
            Exit::from(2)
        }
    }
}

pub(super) fn drift(root: &Path, live: &Path) -> Exit {
    match ruleset_drift(root, live) {
        Ok(drifts) if drifts.is_empty() => Exit::SUCCESS,
        Ok(drifts) => {
            for drift in drifts {
                eprintln!("{drift}");
            }
            Exit::from(1)
        }
        Err(error) => {
            eprintln!("velnor-gen: {error}");
            Exit::from(2)
        }
    }
}

fn ruleset_drift(root: &Path, live: &Path) -> Result<Vec<String>, Error> {
    let root = toplevel(root)?;
    let view = read_tracked(&root)?;
    let live =
        std::fs::read_to_string(live).map_err(|error| Error::File(live.to_path_buf(), error))?;
    release_ruleset_drift(&view, &live).map_err(Error::Generate)
}

fn ruleset(root: &Path) -> Result<String, Error> {
    let root = toplevel(root)?;
    let view = read_tracked(&root)?;
    release_ruleset(&view).map_err(Error::Generate)
}

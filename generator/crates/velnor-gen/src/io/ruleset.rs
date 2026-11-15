// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use super::{Error, Exit, read_tracked, toplevel};
use crate::release_ruleset;

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

fn ruleset(root: &Path) -> Result<String, Error> {
    let root = toplevel(root)?;
    let view = read_tracked(&root)?;
    release_ruleset(&view).map_err(Error::Generate)
}

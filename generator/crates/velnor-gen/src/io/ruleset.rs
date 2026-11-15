// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::Path;

use velnor_model::RepoView;
use velnor_rust::wanted_files;

use super::tracked::tracked;
use super::{Error, Exit, read_files, toplevel};
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
    let tracked_paths = tracked(&root)?;
    let view = RepoView {
        files: read_files(&root, &wanted_files(&tracked_paths))?,
        tracked_paths,
        rust_metadata: BTreeMap::new(),
    };
    release_ruleset(&view).map_err(Error::Generate)
}

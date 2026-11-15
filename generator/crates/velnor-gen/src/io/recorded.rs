// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use velnor_rust::{METADATA_PATH, metadata_file, recorded_metadata};

use super::{
    Checkout, Error, add_manifests, read_files, read_tracked, read_view, replace, toplevel,
};

pub fn write_metadata(path: &Path) -> Result<(), Error> {
    let root = toplevel(path)?;
    let view = read_view(&root, false)?.0;
    let text = metadata_file(&view).map_err(Error::MetadataFile)?;
    replace(&root.join(METADATA_PATH.as_str()), text.as_bytes())
}

pub fn read_recorded_checkout(path: &Path) -> Result<Checkout, Error> {
    let root = toplevel(path)?;
    let mut view = read_tracked(&root)?;
    add_manifests(&mut view, |paths| read_files(&root, paths))?;
    let mut recorded = read_files(&root, &[METADATA_PATH])?;
    view.rust_metadata = recorded_metadata(&view, recorded.remove(&METADATA_PATH).as_deref())
        .map_err(Error::MetadataFile)?;
    Ok(Checkout { root, view })
}

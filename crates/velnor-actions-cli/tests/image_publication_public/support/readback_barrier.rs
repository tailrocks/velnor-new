// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs, path::PathBuf};

use serde_json::{Value, json};

use super::Fixture;

pub(crate) fn configure(
    fixture: &Fixture,
    digest: &str,
) -> Result<(PathBuf, PathBuf), Box<dyn Error>> {
    let mut state: Value = serde_json::from_slice(&fs::read(&fixture.state)?)?;
    state["crane"]["blob_readback_barrier"] = json!({"digest":digest});
    fs::write(&fixture.state, serde_json::to_vec(&state)?)?;
    let home = fixture
        .scratch
        .parent()
        .ok_or("publication scratch has no fixture parent")?
        .join("home");
    Ok((
        home.join(".image-publication-readback-ready"),
        home.join(".image-publication-readback-release"),
    ))
}

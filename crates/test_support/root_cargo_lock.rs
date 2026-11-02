// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::Path;

pub(crate) fn write_root_cargo_lock(root: &Path, packages: &[&str]) -> Result<(), Box<dyn Error>> {
    let mut lock = vec!["version = 4\n".to_owned()];
    lock.extend(packages.iter().map(|name| {
        [
            "[[package]]",
            &format!("name = \"{name}\""),
            "version = \"0.1.0\"\n",
        ]
        .join("\n")
    }));
    fs::write(root.join("Cargo.lock"), lock.join("\n"))?;
    Ok(())
}

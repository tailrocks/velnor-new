// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::SystemTime;

use velnor_gen::io::read_checkout;
use velnor_rust::{detect, sha256_hex};

use super::super::units::{UnitFile, unit_files, workspace};
use super::inventory::{self, Entries};
use crate::cli::InputsBuildArgs;
use crate::error::CiError;

pub(crate) fn build(directory: &Path, args: &InputsBuildArgs) -> Result<String, CiError> {
    let checkout = read_checkout(directory).map_err(|e| CiError::new(e.to_string()))?;
    let facts = detect(&checkout.view).map_err(|e| CiError::new(e.to_string()))?;
    let workspaces = facts
        .rust_roots
        .iter()
        .map(|root| workspace(&checkout.root.join(root.manifest.as_str())))
        .collect::<Result<Vec<_>, _>>()?;
    let targets = inventory::exclusions(&workspaces);
    let mut roots = vec![checkout.root.clone()];
    roots.extend(args.directory.iter().map(|root| directory.join(root)));
    let mut entries = Entries::new();
    let mut remaining = inventory::LIMIT * 10;
    let mut complete = true;
    for root in roots {
        if let Ok(found) = inventory::scan(&root, &targets, &mut remaining) {
            entries.extend(found);
        } else {
            complete = false;
            break;
        }
    }
    let mut before = BTreeMap::new();
    for root in &workspaces {
        for (unit, path) in unit_files(&root.target_directory)? {
            if matches!(
                unit,
                UnitFile::BuildOutput { .. } | UnitFile::DepInfo { .. } | UnitFile::Script { .. }
            ) {
                before.insert(path.clone(), identity(&path)?);
            }
        }
    }
    let started = SystemTime::now();
    let status = Command::new(&args.command[0])
        .args(&args.command[1..])
        .current_dir(directory)
        .status()
        .map_err(|e| CiError::new(format!("cannot run build command: {e}")))?;
    let bytes = serde_json::to_vec(&entries).map_err(|e| CiError::new(e.to_string()))?;
    let digest = sha256_hex(&bytes);
    for root in &workspaces {
        let store = root.target_directory.join(inventory::STORE);
        let saved = store.join(format!("{digest}.json"));
        if complete && status.success() {
            fs::create_dir_all(&store)
                .and_then(|()| fs::write(&saved, &bytes))
                .map_err(|e| CiError::new(format!("cannot save input inventory: {e}")))?;
        }
        for (unit, output) in unit_files(&root.target_directory)? {
            if !matches!(
                unit,
                UnitFile::BuildOutput { .. } | UnitFile::DepInfo { .. } | UnitFile::Script { .. }
            ) {
                continue;
            }
            let current = identity(&output)?;
            if before.get(&output) == Some(&current) {
                continue;
            }
            let receipt = inventory::receipt_path(&output);
            if complete && status.success() && current.1 >= started {
                let record = inventory::receipt(&output, saved.clone(), digest.clone())
                    .map_err(|e| CiError::new(e.to_string()))?;
                let text = serde_json::to_vec(&record).map_err(|e| CiError::new(e.to_string()))?;
                fs::write(&receipt, text)
                    .map_err(|e| CiError::new(format!("cannot save {}: {e}", receipt.display())))?;
            } else if receipt.exists() {
                fs::remove_file(&receipt).map_err(|e| {
                    CiError::new(format!("cannot invalidate {}: {e}", receipt.display()))
                })?;
            }
        }
    }
    if !status.success() {
        return Err(CiError::new(format!("build command failed: {status}")));
    }
    Ok(String::new())
}

fn identity(path: &Path) -> Result<(String, SystemTime), CiError> {
    let bytes = fs::read(path).map_err(|e| CiError::new(e.to_string()))?;
    let modified = fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .map_err(|e| CiError::new(e.to_string()))?;
    Ok((sha256_hex(&bytes), modified))
}

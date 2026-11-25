// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{RepoView, RequiredFeature, TargetKind};
use velnor_rust::{LOCK_PATH, metadata_file, recorded_metadata};

use crate::facts::{TestResult, package, path};
use crate::pipeline::{HELPER_LOCK, view};

fn gated(version: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut files = view(&["Cargo.toml"], None)?;
    let lock = String::from_utf8_lossy(HELPER_LOCK).replace("0.1.81", version);
    files.files.insert(LOCK_PATH, lock.into_bytes());
    let mut app = package("app", "Cargo.toml", &[], &[("app", TargetKind::Bin, false)])?;
    app.targets[0].requires_features = true;
    app.targets[0].required_features = vec![RequiredFeature {
        name: "tls".to_owned(),
        enabled_by: vec!["default".to_owned(), "full".to_owned()],
    }];
    files
        .rust_metadata
        .get_mut(&path("Cargo.toml")?)
        .ok_or("root")?
        .members = vec![app];
    files.tracked_paths.insert(path("Cargo.toml")?);
    files
        .files
        .insert(path("Cargo.toml")?, b"# Cargo.toml\n".to_vec());
    Ok(files)
}

#[test]
fn the_names_of_required_features_are_recorded_only_for_a_pin_that_reads_them() -> TestResult {
    let old = gated("0.1.115")?;
    let text = metadata_file(&old)?;
    assert!(text.contains("\"requires_features\": true"));
    assert!(!text.contains("required_features"));
    let read = recorded_metadata(&old, Some(text.as_bytes()))?;
    let target = &read[&path("Cargo.toml")?].members[0].targets[0];
    assert!(target.requires_features && target.required_features.is_empty());
    let new = gated("0.1.116")?;
    let text = metadata_file(&new)?;
    assert!(text.contains("\"enabled_by\""));
    assert_eq!(
        recorded_metadata(&new, Some(text.as_bytes()))?,
        new.rust_metadata
    );
    assert_eq!(metadata_file(&new)?, text);
    Ok(())
}

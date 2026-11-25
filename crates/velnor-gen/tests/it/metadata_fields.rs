// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{RepoView, RequiredFeature};
use velnor_rust::recorded_metadata;

use crate::facts::{TestResult, path};
use crate::metadata_file::{pin, recorded, two_workspaces};

fn gated(files: &mut RepoView) -> TestResult {
    let root = files
        .rust_metadata
        .get_mut(&path("Cargo.toml")?)
        .ok_or("root")?;
    let app = &mut root.members[0];
    app.features
        .insert("default".to_owned(), vec!["dep:helper".to_owned()]);
    app.dependencies[0].default_features = false;
    let target = &mut app.targets[0];
    target.requires_features = true;
    target.required_features = vec![RequiredFeature {
        name: "extra".to_owned(),
        enabled_by: Vec::new(),
    }];
    target.doc = false;
    target.bench = false;
    Ok(())
}

#[test]
fn a_pin_that_reads_the_target_fields_gets_them_back_exactly() -> TestResult {
    let mut files = two_workspaces()?;
    pin(&mut files, "0.1.117");
    gated(&mut files)?;
    let text = recorded(&files)?;
    let known = recorded_metadata(&files, Some(text.as_bytes()))?;
    assert_eq!(known, files.rust_metadata);
    Ok(())
}

#[test]
fn a_pin_older_than_the_target_fields_gets_the_file_it_can_read() -> TestResult {
    let mut files = two_workspaces()?;
    pin(&mut files, "0.1.115");
    gated(&mut files)?;
    let text = recorded(&files)?;
    for key in [
        "\"doc\"",
        "\"bench\"",
        "\"required_features\"",
        "\"default_features\"",
        "\"features\": {",
    ] {
        assert!(!text.contains(key), "{key} in {text}");
    }
    assert!(text.contains("\"requires_features\": true"), "{text}");
    let known = recorded_metadata(&files, Some(text.as_bytes()))?;
    let target = &known[&path("Cargo.toml")?].members[0].targets[0];
    assert!(target.requires_features);
    Ok(())
}

#[test]
fn a_pin_that_reads_required_features_but_not_the_target_fields_gets_the_names_only() -> TestResult {
    let mut files = two_workspaces()?;
    pin(&mut files, "0.1.116");
    gated(&mut files)?;
    let text = recorded(&files)?;
    for key in ["\"doc\"", "\"bench\"", "\"default_features\"", "\"features\": {"] {
        assert!(!text.contains(key), "{key} in {text}");
    }
    assert!(text.contains("\"required_features\""), "{text}");
    Ok(())
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::json;
use std::collections::BTreeMap;
use velnor_ci::cargo_artifacts::{CargoProgress, ProgressKind};
use velnor_ci::observed_units::{collect, diagnostics};
use velnor_ci::progress_evidence::{Evidence, corroborate};
use velnor_ci::telemetry::Invocation;

fn invocation() -> Invocation {
    let mut artifact = crate::cargo_artifacts::record(false);
    artifact["filenames"] = json!(["target/debug/deps/libapp-opaque.rlib"]);
    serde_json::from_value(json!({"argv":["cargo","build"],"root":".","configuration":"default","started_at":0,"seconds":0.0,"log_start":0,"log_end":0,"compilations":[],"downloads":[],"artifacts":[artifact]})).unwrap_or_else(|error| unreachable!("{error}"))
}

#[test]
fn mixed_output_copies_preserve_the_qualified_compilation_identity() {
    let mut invocation = invocation();
    let mut units = BTreeMap::new();
    collect(&invocation, &mut units);
    invocation.artifacts[0]
        .filenames
        .push("target/debug/libapp.rlib".to_owned());
    collect(&invocation, &mut units);
    assert_eq!(units.len(), 1);
    assert_eq!(units.values().next().expect("unit").1, 2);
    invocation.artifacts[0].fresh = true;
    collect(&invocation, &mut units);
    assert_eq!(units.values().next().expect("unit").1, 2);
    for filename in ["target/debug/libapp.rlib", "target/debug/deps/libapp.rlib"] {
        invocation.artifacts[0].fresh = false;
        invocation.artifacts[0].filenames = vec![filename.to_owned()];
        let mut unknown = BTreeMap::new();
        collect(&invocation, &mut unknown);
        assert!(unknown.is_empty());
        assert_eq!(diagnostics(&invocation).len(), 1);
    }
}

#[test]
fn canonical_documentation_outputs_identify_repeats_and_keep_purposes_distinct() {
    let mut invocation = invocation();
    invocation.artifacts[0].filenames = vec!["target/doc/app/index.html".to_owned()];
    let mut units = BTreeMap::new();
    for _ in 0..3 {
        collect(&invocation, &mut units);
    }
    assert_eq!(diagnostics(&invocation), Vec::<String>::new());
    assert_eq!(units.len(), 1);
    assert_eq!(units.values().next().expect("doc unit").1, 3);
    let mut features = invocation.clone();
    features.artifacts[0].features.push("extra".to_owned());
    collect(&features, &mut units);
    let mut profile = invocation.clone();
    profile.artifacts[0].profile.opt_level = "3".to_owned();
    collect(&profile, &mut units);
    invocation.artifacts[0].filenames = vec!["target/debug/deps/libapp-opaque.rmeta".to_owned()];
    collect(&invocation, &mut units);
    assert_eq!(units.len(), 4);
    assert_eq!(units.values().filter(|(_, count)| *count == 1).count(), 3);
}

#[test]
fn git_progress_matches_the_repository_and_explicit_revision_without_path_confusion() {
    let mut invocation = invocation();
    invocation.artifacts[0].package_id = "git+file:///repo#app@0.1.0".to_owned();
    let mut event = CargoProgress {
        kind: ProgressKind::Compiling,
        package: "app".to_owned(),
        version: "0.1.0".to_owned(),
        location: Some("file:///repo#abcdef12".to_owned()),
    };
    assert_eq!(corroborate(&invocation, &event), Evidence::Compiled);
    event.location = Some("file:///other#abcdef12".to_owned());
    assert_eq!(corroborate(&invocation, &event), Evidence::Unavailable);
    invocation.artifacts[0].package_id = "git+file:///repo?rev=abcdef123456#app@0.1.0".to_owned();
    event.location = Some("file:///repo?rev=abcdef123456#abcdef12".to_owned());
    assert_eq!(corroborate(&invocation, &event), Evidence::Compiled);
    event.location = Some("file:///repo?rev=abcdef123456#12345678".to_owned());
    assert_eq!(corroborate(&invocation, &event), Evidence::Unavailable);
    invocation.artifacts[0].package_id = "path+file:///工作/app#0.1.0".to_owned();
    event.location = Some("/工作/app".to_owned());
    assert_eq!(corroborate(&invocation, &event), Evidence::Compiled);
    event.location = Some("/工作/other".to_owned());
    assert_eq!(corroborate(&invocation, &event), Evidence::Unavailable);
}

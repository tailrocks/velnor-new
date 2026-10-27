// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::error::Error;

use velnor_actions_rust::closure::resolve_dependency_closure;

fn rejects_unknown_units() {
    let (graph, workspaces, lock, features, targets, native_inputs) = base_setup();
    assert!(
        dependency_coverage(&DependencyCoverageInputs {
            package_id: "",
            graph: &graph,
            workspaces: &workspaces,
            features: &features,
            all_features: false,
            targets: &targets,
            lock_digest: &lock,
            native_inputs: &native_inputs,
        })
        .is_err()
    );
    assert!(
        dependency_coverage(&DependencyCoverageInputs {
            package_id: "missing-crate",
            graph: &graph,
            workspaces: &workspaces,
            features: &features,
            all_features: false,
            targets: &targets,
            lock_digest: &lock,
            native_inputs: &native_inputs,
        })
        .is_err()
    );
}

fn rejects_bad_lockfile() {
    let (graph, workspaces, _, features, targets, native_inputs) = base_setup();
    let unknown = DigestSlot::Unknown("lockfile_unreadable".to_owned());
    assert!(
        dependency_coverage(&fixture_inputs(
            &graph,
            &workspaces,
            &unknown,
            &features,
            &targets,
            &native_inputs,
        ))
        .is_err()
    );
    let malformed = DigestSlot::Known("not-a-digest".to_owned());
    assert!(
        dependency_coverage(&fixture_inputs(
            &graph,
            &workspaces,
            &malformed,
            &features,
            &targets,
            &native_inputs,
        ))
        .is_err()
    );
}

fn rejects_bad_entries() {
    let (graph, workspaces, lock, _, targets, native_inputs) = base_setup();
    let empty_feature = vec![String::new()];
    assert!(
        dependency_coverage(&fixture_inputs(
            &graph,
            &workspaces,
            &lock,
            &empty_feature,
            &targets,
            &native_inputs,
        ))
        .is_err()
    );
    let features = vec!["serde".to_owned()];
    let empty_native = vec![String::new()];
    assert!(
        dependency_coverage(&fixture_inputs(
            &graph,
            &workspaces,
            &lock,
            &features,
            &targets,
            &empty_native,
        ))
        .is_err()
    );
    let backslash_native = vec!["vendor\\lib".to_owned()];
    assert!(
        dependency_coverage(&fixture_inputs(
            &graph,
            &workspaces,
            &lock,
            &features,
            &targets,
            &backslash_native,
        ))
        .is_err()
    );
}

fn rejects_bad_closure(base: &DependencyCoverage) {
    assert!(resolve_dependency_closure("", base).is_err());
    let mut tampered = base.clone();
    "not-a-digest".clone_into(&mut tampered.coverage_digest);
    assert!(resolve_dependency_closure("rust-app-build", &tampered).is_err());
    let mut tampered = base.clone();
    "b3-ABCD".clone_into(&mut tampered.native_digest);
    assert!(resolve_dependency_closure("rust-app-build", &tampered).is_err());
}

#[test]
fn gc1_t06_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    let base = base_coverage()?;
    rejects_unknown_units();
    rejects_bad_lockfile();
    rejects_bad_entries();
    rejects_bad_closure(&base);
    Ok(())
}

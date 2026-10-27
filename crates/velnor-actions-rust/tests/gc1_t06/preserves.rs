// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::error::Error;

use velnor_actions_contract::canonical::Digest;
use velnor_actions_rust::closure::resolve_dependency_closure;
use velnor_actions_rust::identity::GroupExtensionInputs;

fn preserves_determinism() -> Result<(), Box<dyn Error>> {
    let (graph, workspaces, lock, features, targets, native_inputs) = base_setup();
    let first = dependency_coverage(&fixture_inputs(
        &graph,
        &workspaces,
        &lock,
        &features,
        &targets,
        &native_inputs,
    ))?;
    let second = dependency_coverage(&fixture_inputs(
        &graph,
        &workspaces,
        &lock,
        &features,
        &targets,
        &native_inputs,
    ))?;
    assert_eq!(first, second);
    for digest in [
        &first.dependency_digest,
        &first.workspace_digest,
        &first.native_digest,
        &first.coverage_digest,
    ] {
        Digest::parse(digest)?;
    }
    Ok(())
}

fn preserves_input_order() -> Result<(), Box<dyn Error>> {
    let (graph, workspaces, lock, _, _, _) = base_setup();
    let features = vec!["serde".to_owned(), "tokio".to_owned()];
    let reversed_features = vec!["tokio".to_owned(), "serde".to_owned(), "serde".to_owned()];
    let targets = vec!["aarch64-linux".to_owned(), "x86_64-linux".to_owned()];
    let reversed_targets = vec!["x86_64-linux".to_owned(), "aarch64-linux".to_owned()];
    let native_inputs = vec!["libgit2".to_owned(), "openssl-src".to_owned()];
    let reversed_native = vec!["openssl-src".to_owned(), "libgit2".to_owned()];
    let first = dependency_coverage(&fixture_inputs(
        &graph,
        &workspaces,
        &lock,
        &features,
        &targets,
        &native_inputs,
    ))?;
    let second = dependency_coverage(&fixture_inputs(
        &graph,
        &workspaces,
        &lock,
        &reversed_features,
        &reversed_targets,
        &reversed_native,
    ))?;
    assert_eq!(first, second);
    Ok(())
}

fn preserves_application_only_delta() -> Result<(), Box<dyn Error>> {
    let graph = fixture_graph();
    let before_workspaces = fixture_workspaces("0.1.0", "0.2.0");
    let after_workspaces = fixture_workspaces("0.1.1", "0.2.0");
    let lock = DigestSlot::Known(digest_fill('f'));
    let features = vec!["serde".to_owned()];
    let targets = vec!["x86_64-unknown-linux-gnu".to_owned()];
    let native_inputs = vec!["openssl-src".to_owned()];
    let before = dependency_coverage(&fixture_inputs(
        &graph,
        &before_workspaces,
        &lock,
        &features,
        &targets,
        &native_inputs,
    ))?;
    let after = dependency_coverage(&fixture_inputs(
        &graph,
        &after_workspaces,
        &lock,
        &features,
        &targets,
        &native_inputs,
    ))?;
    assert_eq!(before, after);
    Ok(())
}

fn preserves_extension_separation() -> Result<(), Box<dyn Error>> {
    let group = fixture_group();
    let graph = fixture_graph();
    let workspaces = fixture_workspaces("0.1.0", "0.2.0");
    let config = digest_fill('e');
    let extension = group.identity_extension(&GroupExtensionInputs {
        package_id: APP,
        workspace_id: "workspace-1",
        profile: "dev",
        manifest: "crates/app/Cargo.toml",
        graph_digest: &digest_fill('d'),
        targets: &["x86_64-unknown-linux-gnu".to_owned()],
        config_digest: &config,
        lock_digest: DigestSlot::Known(digest_fill('f')),
        nextest_digest: DigestSlot::AbsentProven("probed_no_nextest_config".to_owned()),
        archive_source: None,
        rerun_inputs: None,
        has_build_script: false,
    });
    let native_inputs = vec!["openssl-src".to_owned()];
    let from_extension = extension.dependency_coverage(APP, &graph, &workspaces, &native_inputs)?;
    let direct = base_coverage()?;
    assert_eq!(from_extension, direct);
    let closure = resolve_dependency_closure("rust-app-build", &from_extension)?;
    assert_eq!(closure.task_id, "rust-app-build");
    let mut keys: Vec<&str> = closure.inputs.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec![
            "coverage",
            "dependencies",
            "native_inputs",
            "workspace_deps"
        ]
    );
    closure.verify_complete()?;
    Ok(())
}

fn preserves_role_changes() -> Result<(), Box<dyn Error>> {
    let base = base_coverage()?;
    let graph = fixture_graph();
    let workspaces = fixture_workspaces("0.1.0", "0.2.0");
    let lock = DigestSlot::Known(digest_fill('f'));
    let targets = vec!["x86_64-unknown-linux-gnu".to_owned()];
    let native_inputs = vec!["openssl-src".to_owned()];
    let changed_features = vec!["serde".to_owned(), "tokio".to_owned()];
    let feature_change = dependency_coverage(&fixture_inputs(
        &graph,
        &workspaces,
        &lock,
        &changed_features,
        &targets,
        &native_inputs,
    ))?;
    assert_ne!(feature_change.dependency_digest, base.dependency_digest);
    assert_eq!(feature_change.workspace_digest, base.workspace_digest);
    assert_eq!(feature_change.native_digest, base.native_digest);
    assert_ne!(feature_change.coverage_digest, base.coverage_digest);
    let changed_native = vec!["openssl-src".to_owned(), "libgit2".to_owned()];
    let base_features = vec!["serde".to_owned()];
    let native_change = dependency_coverage(&fixture_inputs(
        &graph,
        &workspaces,
        &lock,
        &base_features,
        &targets,
        &changed_native,
    ))?;
    assert_eq!(native_change.dependency_digest, base.dependency_digest);
    assert_eq!(native_change.workspace_digest, base.workspace_digest);
    assert_ne!(native_change.native_digest, base.native_digest);
    assert_ne!(native_change.coverage_digest, base.coverage_digest);
    let changed_workspaces = fixture_workspaces("0.1.0", "0.3.0");
    let workspace_change = dependency_coverage(&fixture_inputs(
        &graph,
        &changed_workspaces,
        &lock,
        &base_features,
        &targets,
        &native_inputs,
    ))?;
    assert_eq!(workspace_change.dependency_digest, base.dependency_digest);
    assert_ne!(workspace_change.workspace_digest, base.workspace_digest);
    assert_ne!(workspace_change.coverage_digest, base.coverage_digest);
    let changed_lock = DigestSlot::Known(digest_fill('1'));
    let lock_change = dependency_coverage(&fixture_inputs(
        &graph,
        &workspaces,
        &changed_lock,
        &base_features,
        &targets,
        &native_inputs,
    ))?;
    assert_ne!(lock_change.dependency_digest, base.dependency_digest);
    assert_ne!(lock_change.coverage_digest, base.coverage_digest);
    Ok(())
}

#[test]
fn gc1_t06_preserves_contract() -> Result<(), Box<dyn Error>> {
    preserves_determinism()?;
    preserves_input_order()?;
    preserves_application_only_delta()?;
    preserves_extension_separation()?;
    preserves_role_changes()?;
    Ok(())
}

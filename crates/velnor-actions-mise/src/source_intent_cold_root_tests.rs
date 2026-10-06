use super::*;
use velnor_actions_contract::CacheSnapshotDomain;

#[test]
fn cold_intent_selects_exact_root_linux_profile() {
    let root = SourceIntentColdRoot::root_linux();
    assert_eq!(root.compiler_version(), "1.98.1");
    assert_eq!(root.host().target_triple(), "x86_64-unknown-linux-gnu");
    assert_eq!(root.options().components(), ["clippy", "rustfmt"]);
    assert!(root.options().targets().is_empty());
    assert_eq!(
        root.options()
            .tool_spec(root.compiler_version())
            .expect("exact root pin"),
        "rust[profile=minimal,components=clippy,rustfmt]@1.98.1"
    );
}

#[test]
fn cold_root_has_one_fixed_binding_and_closed_leaves() {
    let root = SourceIntentColdRoot::root_linux();
    assert_eq!(SourceIntentColdLeaf::Rustup.relative(), "rustup-home");
    assert_eq!(
        root.namespace_environment(),
        (
            "VELNOR_SOURCE_INTENT_COLD_ROOT",
            "${{ runner.temp }}/velnor-control/source-intent"
        )
    );
    assert_eq!(root.runner_temp_environment(), "RUNNER_TEMP");
    assert_eq!(
        root.namespace_environment().1,
        format!("${{{{ runner.temp }}}}/{}", root.relative_to_runner_temp())
    );
    for (index, leaf) in root.leaves().iter().enumerate() {
        assert!(!leaf.relative().contains('/'));
        assert!(!leaf.relative().is_empty());
        assert_eq!(
            root.leaf_expression(*leaf),
            format!(
                "${{{{ runner.temp }}}}/velnor-control/source-intent/{}",
                leaf.relative()
            )
        );
        for other in &root.leaves()[index + 1..] {
            assert_ne!(leaf.relative(), other.relative());
        }
    }
}

#[test]
fn cold_sdk_never_aliases_any_cached_payload() {
    let root = SourceIntentColdRoot::root_linux();
    for domain in CacheSnapshotDomain::ALL {
        for cached in domain.roots() {
            let cached = format!("${{{{ runner.temp }}}}/velnor/{cached}");
            for leaf in root.leaves() {
                let cold = root.leaf_expression(*leaf);
                assert_ne!(cold, cached);
                assert!(!cold.starts_with(&format!("{cached}/")));
                assert!(!cached.starts_with(&format!("{cold}/")));
            }
        }
    }
}

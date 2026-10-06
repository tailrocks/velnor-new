use super::*;

#[test]
fn candidate_selects_exact_root_linux_profile() {
    let root = RootRustCandidateRoot::root_linux();
    assert_eq!(root.compiler_version(), "1.98.1");
    assert_eq!(root.host(), RustHost::LinuxAmd64);
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
fn candidate_has_one_fixed_binding_and_five_closed_leaves() {
    let root = RootRustCandidateRoot::root_linux();
    assert_eq!(
        root.namespace_environment(),
        (
            "VELNOR_ROOT_RUST_CANDIDATE_ROOT",
            "${{ runner.temp }}/velnor-control/root-rust-candidate"
        )
    );
    assert_eq!(root.runner_temp_environment(), "RUNNER_TEMP");
    assert_eq!(
        root.namespace_environment().1,
        format!("${{{{ runner.temp }}}}/{}", root.relative_to_runner_temp())
    );
    assert_eq!(root.leaves().len(), 5);
    assert_eq!(RootRustCandidateLeaf::CargoHome.relative(), "cargo-home");
    assert_eq!(RootRustCandidateLeaf::RustupHome.relative(), "rustup-home");
    assert_eq!(
        RootRustCandidateLeaf::RustupBootstrap.relative(),
        "rustup-bootstrap"
    );
    assert_eq!(RootRustCandidateLeaf::NativeDist.relative(), "native-dist");
    assert_eq!(RootRustCandidateLeaf::ManagerBin.relative(), "manager-bin");
    for (index, leaf) in root.leaves().iter().enumerate() {
        assert!(!leaf.relative().contains('/'));
        assert!(!leaf.relative().is_empty());
        assert_eq!(
            root.leaf_expression(*leaf),
            format!(
                "${{{{ runner.temp }}}}/velnor-control/root-rust-candidate/{}",
                leaf.relative()
            )
        );
        for other in &root.leaves()[index + 1..] {
            assert_ne!(leaf.relative(), other.relative());
        }
    }
}

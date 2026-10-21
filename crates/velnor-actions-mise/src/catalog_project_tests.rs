use super::*;

fn declared() -> ToolCatalog {
    ToolCatalog::pinned()
        .with_project_tool(PinnedTool::Rust, "1.42.7")
        .and_then(|catalog| catalog.with_project_tool(PinnedTool::Nextest, "0.9.1"))
        .expect("exact project pins")
}

#[test]
fn project_tool_keeps_its_concrete_version() {
    let catalog = declared();
    assert_eq!(catalog.version(PinnedTool::Rust), "1.42.7");
    assert!(catalog.is_project_declared(PinnedTool::Rust));
    assert!(!catalog.is_project_declared(PinnedTool::MrBoxington));
    assert!(!catalog.is_emission());
    assert_eq!(
        catalog.materialize("rust@${VELNOR_TOOL_RUST}"),
        "rust@1.42.7"
    );
}

#[test]
fn emission_replaces_only_project_versions() {
    let emitted = declared().for_emission();
    assert!(emitted.is_emission());
    assert_eq!(
        emitted.tool_spec(PinnedTool::Rust),
        "rust@${VELNOR_TOOL_RUST}"
    );
    assert_eq!(
        emitted.tool_spec(PinnedTool::Nextest),
        "aqua:nextest-rs/nextest/cargo-nextest@${VELNOR_TOOL_NEXTEST}"
    );
    assert_eq!(
        emitted.tool_spec(PinnedTool::MrBoxington),
        format!("mr-boxington@{}", crate::catalog::MR_BOXINGTON_VERSION)
    );
    assert_eq!(
        emitted.rust_toolchain_name(),
        "${VELNOR_TOOL_RUST}-x86_64-unknown-linux-gnu"
    );
    // An emission catalog never materializes back to a literal.
    assert_eq!(
        emitted.materialize("${VELNOR_TOOL_RUST}"),
        "${VELNOR_TOOL_RUST}"
    );
}

#[test]
fn project_version_must_be_exact() {
    assert!(
        ToolCatalog::pinned()
            .with_project_tool(PinnedTool::Rust, "1.42")
            .is_err()
    );
    assert!(
        ToolCatalog::pinned()
            .with_project_tool(PinnedTool::Rust, "${VELNOR_TOOL_RUST}")
            .is_err()
    );
}

#[test]
fn reference_helpers_round_trip() {
    let text = "rust@${VELNOR_TOOL_RUST}-x";
    assert!(has_project_tool_ref(text));
    assert!(is_project_tool_ref("${VELNOR_TOOL_RUST}"));
    assert!(!is_project_tool_ref("1.2.3"));
    assert_eq!(without_project_tool_refs(text), "rust@0-x");
    assert_eq!(
        project_tool_refs_to_expressions(text),
        "rust@${{ env.VELNOR_TOOL_RUST }}-x"
    );
}

#[test]
fn every_tool_has_a_distinct_project_env_and_keys() {
    let mut names: Vec<_> = PinnedTool::ALL.iter().map(|t| t.project_env()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), PinnedTool::ALL.len());
    assert!(
        names
            .iter()
            .all(|name| name.starts_with(PROJECT_TOOL_ENV_PREFIX))
    );
    for tool in PinnedTool::ALL {
        assert!(!tool.mise_config_keys().is_empty(), "{tool:?}");
    }
    assert_eq!(
        PinnedTool::Nextest.mise_config_keys()[0],
        "aqua:nextest-rs/nextest/cargo-nextest"
    );
}

#[test]
fn a_config_pin_replaces_the_velnor_pin_without_declaring_the_tool() {
    let catalog = ToolCatalog::pinned()
        .with_version(PinnedTool::Jactionlint, "2.4.0")
        .expect("exact pin");
    assert_eq!(catalog.version(PinnedTool::Jactionlint), "2.4.0");
    assert!(!catalog.is_project_declared(PinnedTool::Jactionlint));
    assert_eq!(
        catalog.for_emission().tool_spec(PinnedTool::Jactionlint),
        "jactionlint@2.4.0"
    );
    for loose in ["2.4", "v2.4.0", "latest", "${VELNOR_TOOL_JACTIONLINT}"] {
        assert!(
            ToolCatalog::pinned()
                .with_version(PinnedTool::Jactionlint, loose)
                .is_err(),
            "{loose}"
        );
    }
}

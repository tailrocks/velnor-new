//! Exact normalized Java selectors and vendor release grammar.
use velnor_actions_mise::{PinnedTool, validate_exact_version};

#[test]
fn java_pins_preserve_vendor_elements_and_normalized_initial_releases() {
    for version in ["25.0.0", "25.0.3", "25.0.4.1", "25.0.4.1.1", "25.0.4.1.1.2"] {
        assert!(validate_exact_version("java", version).is_ok(), "{version}");
    }
}

#[test]
fn java_pins_reject_loose_and_noncanonical_numeric_selectors() {
    for version in [
        "25",
        "25.0",
        "latest",
        "v25.0.3",
        "oracle-graalvm-25.0.3",
        "",
        "25..3",
        "25.0.4.",
        "25.0.4.1.x",
        "25.0.3-beta",
        "0.0.3",
        "025.0.3",
        "25.00.3",
        "25.0.04.1.1",
        "25.0.4.01.1",
    ] {
        assert!(
            validate_exact_version("java", version).is_err(),
            "{version}"
        );
    }
}

#[test]
fn extended_java_version_grammar_does_not_relax_other_tools() {
    for tool in PinnedTool::ALL
        .into_iter()
        .filter(|tool| *tool != PinnedTool::Java)
    {
        for version in ["25.0.4.1", "25.0.4.1.1"] {
            assert!(validate_exact_version(tool.tool_name(), version).is_err());
        }
    }
}

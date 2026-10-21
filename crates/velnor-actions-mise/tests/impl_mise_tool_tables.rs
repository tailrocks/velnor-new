//! MBX pin reconciliation reads every Mise tool-table spelling.
use velnor_actions_mise::MR_BOXINGTON_VERSION;
use velnor_actions_mise::toolfiles::{MbxWrapperStatus, mbx_wrapper_pin_status};

const WRAPPER: &str = "[wrappers.cargo]\ncommand = \"mbx\"\n";

fn status(tools: &str) -> MbxWrapperStatus {
    mbx_wrapper_pin_status(&format!("{WRAPPER}{tools}"))
}

#[test]
fn mbx_wrapper_reads_scalar_inline_and_nested_tool_pins() {
    let v = MR_BOXINGTON_VERSION;
    for (tools, form) in [
        (format!("[tools]\nmr-boxington = \"{v}\"\n"), "scalar"),
        (
            format!("[tools]\nmr-boxington = {{ version = \"{v}\", os = [\"linux\"] }}\n"),
            "inline table",
        ),
        (
            format!("[tools.mr-boxington]\nversion = \"{v}\"\nos = [\"linux\"]\n"),
            "nested table",
        ),
        (
            format!("[tools]\nmr-boxington.version = \"{v}\"\n"),
            "dotted key",
        ),
        (
            format!("[tools]\nmr-boxington = \"{v}\" # pinned\n[env]\nX = \"wrong\"\n"),
            "comment and unrelated section",
        ),
        (
            format!(
                "[tools]\nmr-boxington = {{ version = \"{v}\", workflow = \"release.yml\" }}\n[env]\nMR_BOXINGTON_VERSION = \"wrong\"\n"
            ),
            "unrelated environment value",
        ),
    ] {
        assert_eq!(
            status(&tools),
            MbxWrapperStatus::Pinned {
                version: v.to_owned()
            },
            "{form}"
        );
    }
}

#[test]
fn mbx_wrapper_rejects_non_string_and_duplicate_tool_versions() {
    let v = MR_BOXINGTON_VERSION;
    for tools in [
        "[tools]\nmr-boxington = { version = 123 }\n".to_owned(),
        "[tools]\nmr-boxington = { version = true }\n".to_owned(),
        format!("[tools]\nmr-boxington = {{ version = [\"{v}\"] }}\n"),
        "[tools]\nmr-boxington.version = { major = 1 }\n".to_owned(),
        format!("[tools]\nmr-boxington = [\"{v}\"]\n"),
        format!(
            "[tools]\nmr-boxington = {{ version = \"{v}\" }}\nmr-boxington.version = \"{v}\"\n"
        ),
        format!("[tools]\nmr-boxington = \"{v}\"\n[tools.mr-boxington]\nversion = \"{v}\"\n"),
    ] {
        assert!(
            matches!(status(&tools), MbxWrapperStatus::Invalid { .. }),
            "{tools}"
        );
    }
    assert_eq!(
        status("[tools]\nmr-boxington = { os = [\"linux\"] }\n"),
        MbxWrapperStatus::MissingPin,
        "a tool table without a version declares no pin"
    );
}

#[test]
fn mbx_wrapper_rejects_non_version_tool_selectors() {
    for selector in ["path", "prefix", "ref"] {
        let tools = format!("[tools]\nmr-boxington = {{ {selector} = \"value\" }}\n");
        let expected = format!("unsupported_tool_selector:mr-boxington:{selector}");
        assert!(
            matches!(
                status(&tools),
                MbxWrapperStatus::Invalid { problem } if problem.contains(&expected)
            ),
            "{selector}"
        );
    }
    let mixed = format!(
        "[tools]\nmr-boxington = {{ version = \"{MR_BOXINGTON_VERSION}\", path = \"./mbx\" }}\n"
    );
    assert!(matches!(status(&mixed), MbxWrapperStatus::Invalid { .. }));
}

#[test]
fn mbx_wrapper_rejects_tool_array_tables() {
    for tools in [
        format!("[[tools.mr-boxington]]\nversion = \"{MR_BOXINGTON_VERSION}\"\n"),
        "[[tools.mr-boxington]]\nworkflow = \"release.yml\"\n".to_owned(),
        "[[tools.mr-boxington]]\n".to_owned(),
    ] {
        assert!(
            matches!(
                status(&tools),
                MbxWrapperStatus::Invalid { problem }
                    if problem.contains("tool_array_table:tools.mr-boxington")
            ),
            "{tools}"
        );
    }
}

#[test]
fn native_mr_boxington_option_reconciles_like_the_wrapper() {
    for (content, want) in [
        (
            "[tools]\nrust = { version = \"1.99.1\", mr_boxington = true }\nmr-boxington = \"1.26.0\"\n",
            MbxWrapperStatus::Pinned {
                version: "1.26.0".to_owned(),
            },
        ),
        (
            "[tools.rust]\nversion = \"1.99.1\"\nmr_boxington = true\n",
            MbxWrapperStatus::MissingPin,
        ),
        (
            "[tools]\nrust = { version = \"1.99.1\", mr_boxington = false }\n",
            MbxWrapperStatus::NoMbxWrapper,
        ),
        (
            "[tools]\n# rust = { version = \"1.99.1\", mr_boxington = true }\n",
            MbxWrapperStatus::NoMbxWrapper,
        ),
    ] {
        assert_eq!(mbx_wrapper_pin_status(content), want, "{content:?}");
    }
}

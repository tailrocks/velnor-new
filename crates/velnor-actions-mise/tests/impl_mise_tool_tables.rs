//! These tests do a check of Mise tool versions.
use velnor_actions_mise::MR_BOXINGTON_VERSION;
use velnor_actions_mise::toolfiles::{MbxWrapperStatus, mbx_wrapper_pin_status};

#[test]
fn mbx_wrapper_reconciles_scalar_and_tool_table_pins() {
    let cases = [
        (
            "[wrappers.cargo]\ncommand = \"mbx\"\n[tools]\nmr-boxington = \"1.24.1\"\n",
            "scalar",
        ),
        (
            "[wrappers.cargo]\ncommand = \"mbx\"\n[tools]\nmr-boxington = { version = \"1.24.1\", workflow = \"release.yml\" }\n",
            "inline table",
        ),
        (
            "[wrappers.cargo]\ncommand = \"mbx\"\n[tools.mr-boxington]\nversion = \"1.24.1\"\nworkflow = \"release.yml\"\n",
            "nested table",
        ),
        (
            "[wrappers.cargo]\ncommand = \"mbx\"\n[tools]\nmr-boxington = { version = \"1.24.1\", workflow = \"release.yml\" }\n[env]\nMR_BOXINGTON_VERSION = \"wrong\"\n",
            "unrelated section",
        ),
    ];
    for (content, form) in cases {
        let content = content.replace("1.24.1", MR_BOXINGTON_VERSION);
        assert_eq!(
            mbx_wrapper_pin_status(&content),
            MbxWrapperStatus::Pinned {
                version: MR_BOXINGTON_VERSION.to_owned(),
            },
            "{form}: the version must be MR_BOXINGTON_VERSION."
        );
    }
}

#[test]
fn mbx_wrapper_rejects_invalid_tool_table_versions() {
    for tools in [
        "mr-boxington = { version = 1.24.1, workflow = \"release.yml\" }",
        "mr-boxington = { version = true, workflow = \"release.yml\" }",
        "mr-boxington = { version = [\"1.24.1\"] }",
        "mr-boxington.version = { major = 1 }",
        "mr-boxington = { version = \"1.24.1\" }\nmr-boxington.version = \"1.24.2\"",
    ] {
        let content = format!("[wrappers.cargo]\ncommand = \"mbx\"\n[tools]\n{tools}\n");
        assert!(
            matches!(
                mbx_wrapper_pin_status(&content),
                MbxWrapperStatus::Invalid { .. }
            ),
            "The parser must reject this version: {tools}."
        );
    }
    let metadata_only = "[wrappers.cargo]\ncommand = \"mbx\"\n[tools]\nmr-boxington = { workflow = \"release.yml\" }\n";
    assert_eq!(
        mbx_wrapper_pin_status(metadata_only),
        MbxWrapperStatus::MissingPin,
        "The metadata has no version."
    );
}

#[test]
fn mbx_wrapper_rejects_unsupported_tool_selectors() {
    for selector in ["path", "prefix", "ref"] {
        let content = format!(
            "[wrappers.cargo]\ncommand = \"mbx\"\n[tools]\nmr-boxington = {{ {selector} = \"value\" }}\n"
        );
        assert!(
            matches!(
                mbx_wrapper_pin_status(&content),
                MbxWrapperStatus::Invalid { problem } if problem.contains(&format!("unsupported_tool_selector:mr-boxington:{selector}"))
            ),
            "The parser must reject {selector}."
        );
    }

    let conflicting = "[wrappers.cargo]\ncommand = \"mbx\"\n[tools]\nmr-boxington = { version = \"1.24.1\", path = \"./mbx\" }\n";
    assert!(
        matches!(
            mbx_wrapper_pin_status(conflicting),
            MbxWrapperStatus::Invalid { .. }
        ),
        "The parser must reject these fields."
    );
}

#[test]
fn mbx_wrapper_rejects_tool_array_tables() {
    for declaration in [
        "[[tools.mr-boxington]]\nversion = \"1.24.1\"\n",
        "[[tools.mr-boxington]]\nworkflow = \"release.yml\"\n",
        "[[tools.mr-boxington]]\n",
    ] {
        let content = format!("[wrappers.cargo]\ncommand = \"mbx\"\n{declaration}");
        assert!(
            matches!(
                mbx_wrapper_pin_status(&content),
                MbxWrapperStatus::Invalid { problem } if problem.contains("tool_array_table:tools.mr-boxington")
            ),
            "The parser must reject this table: {declaration}."
        );
    }
}

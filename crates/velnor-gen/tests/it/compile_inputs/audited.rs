// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::InputKey;
use velnor_rust::detect;

use super::embed::{
    Evidence, audited, embedding, pinned, selected, selections, unresolved, view_of,
};
use crate::base_save::pinned as generated;
use crate::base_save::pull_request_audit::audit_step_runs;
use crate::facts::{TestResult, lib, repo};
use crate::pipeline::workflow;
use crate::select::path;

const MANIFEST_DIR_FILES: [(&str, &str); 4] = [
    (
        "crates/reader/src/lib.rs",
        "include!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/src/x.rs\"));\ninclude!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../../shared/gen.rs\"));\nconst G: &str = include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../../docs\", \"/guide.md\"));\nconst L: &[u8] = include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../../assets/logo.bin\"));",
    ),
    ("crates/reader/src/x.rs", ""),
    (
        "shared/gen.rs",
        "const D: &str = include_str!(\"../docs/deep.md\");",
    ),
    ("crates/app/src/lib.rs", ""),
];

#[test]
fn a_manifest_dir_include_resolves_to_the_file_it_reads() -> TestResult {
    embedding(
        &MANIFEST_DIR_FILES,
        &[
            ("docs/guide.md", &["reader"]),
            ("docs/deep.md", &["reader"]),
            ("shared/gen.rs", &["reader"]),
            ("assets/logo.bin", &["reader"]),
            ("docs/other.md", &[]),
        ],
        Evidence::Audited,
    )
}

const OUT_DIR_FILES: [(&str, &str); 2] = [
    (
        "crates/reader/src/lib.rs",
        "include!(concat!(env!(\"OUT_DIR\"), \"/gen.rs\"));\nconst B: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/sub\", \"/blob.bin\"));",
    ),
    ("crates/app/src/lib.rs", ""),
];

#[test]
fn an_out_dir_include_is_a_build_output_with_no_input() -> TestResult {
    embedding(&OUT_DIR_FILES, &[("docs/other.md", &[])], Evidence::Audited)
}

#[test]
fn an_include_that_cannot_be_resolved_selects_its_crate_on_any_change_with_reason_unresolved_include()
-> TestResult {
    for source in [
        "include_str!(env!(\"OTHER\"));",
        "include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/src/x.md\", suffix()));",
        "include_str!(path_of!());",
        "include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../../../../x.md\"));",
        "include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"src/x.md\"));",
        "include_str!(concat!(env!(\"OUT_DIR\"), \"/../x.md\"));",
    ] {
        unresolved(source, Evidence::Audited)?;
    }
    let files = [
        ("crates/c/src/lib.rs", "include!(env!(\"OTHER\"));"),
        (
            "crates/app/src/lib.rs",
            "include_str!(\"../../../docs/guide.md\");",
        ),
        ("docs/guide.md", ""),
    ];
    assert_eq!(
        selections(&["app", "c", "tool", "left"], &files, "docs/guide.md")?,
        [
            ("app".to_owned(), "input:docs/guide.md".to_owned()),
            ("c".to_owned(), "unresolved-include".to_owned()),
        ]
    );
    Ok(())
}

#[test]
fn a_root_package_and_a_nested_member_resolve_against_their_own_manifest_dir() -> TestResult {
    let view = repo(
        vec![(
            "Cargo.toml",
            vec![
                lib("top", "Cargo.toml")?,
                lib("inner", "crates/inner/Cargo.toml")?,
            ],
        )],
        &[
            (
                "src/lib.rs",
                "const A: &str = include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/README.md\"));",
            ),
            (
                "crates/inner/src/lib.rs",
                "const B: &str = include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../../docs/inner.md\"));",
            ),
            ("README.md", ""),
            ("docs/inner.md", ""),
        ],
    )?;
    let mut view = view;
    audited(&mut view)?;
    let facts = detect(&view)?;
    let owners = |file: &str| -> Option<Vec<String>> {
        facts
            .compile_inputs
            .get(&InputKey::Path(path(file)))
            .map(|owners| owners.iter().map(|owner| owner.name.to_string()).collect())
    };
    assert_eq!(owners("README.md"), Some(vec!["top".to_owned()]));
    assert_eq!(owners("docs/inner.md"), Some(vec!["inner".to_owned()]));
    assert!(!facts.compile_inputs.contains_key(&InputKey::Unresolved));
    Ok(())
}

const EVERYTHING: &str = "on_main = \"everything\"\n";

const CONFIGURED: [(&str, &str); 2] = [
    (
        "manifest dir",
        "include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../../docs/guide.md\"));",
    ),
    (
        "out dir",
        "include_str!(concat!(env!(\"OUT_DIR\"), \"/gen.md\"));",
    ),
];

#[test]
fn a_manifest_dir_or_out_dir_include_is_trusted_exactly_when_the_generated_audit_runs_on_pull_requests()
-> TestResult {
    for (kind, source) in CONFIGURED {
        for helper in ["0.1.116", "0.1.117"] {
            for config in ["", EVERYTHING] {
                let mut view = view_of(
                    &["app", "reader"],
                    &[
                        ("crates/reader/src/lib.rs", source),
                        ("crates/app/src/lib.rs", ""),
                        ("docs/guide.md", ""),
                    ],
                )?;
                pinned(&mut view, helper)?;
                if !config.is_empty() {
                    let file = path(".velnor/rust-ci.toml");
                    view.tracked_paths.insert(file.clone());
                    view.files.insert(file, config.as_bytes().to_vec());
                }
                let audited = audit_step_runs(
                    &workflow(&generated(helper, config)?)?,
                    "pull_request",
                    false,
                )?;
                assert_eq!(audited, helper == "0.1.117", "{kind} {helper} {config}");
                let trusted = selected(&view, "docs/other.md")?.is_empty();
                assert_eq!(
                    trusted, audited,
                    "{kind} {helper} {config}: the plan trusts the include only where the audit runs"
                );
                if !audited {
                    assert_eq!(
                        selected(&view, "docs/other.md")?,
                        [("reader".to_owned(), "unresolved-include".to_owned())],
                        "{kind} {helper} {config}"
                    );
                }
            }
        }
    }
    Ok(())
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{RepoView, Target, TargetKind, TargetName};

use super::embed::{selected, view_of};
use crate::facts::{TestResult, path};

pub(super) const DECLARES: &str = "fn main() { println!(\"cargo:rerun-if-changed=build.rs\"); }";
pub(super) const OUT: &str = "include!(concat!(env!(\"OUT_DIR\"), \"/generated.rs\"));\nconst B: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/blob.bin\"));";
pub(super) const UNRESOLVED: [(&str, &str); 1] = [("c", "unresolved-include")];

pub(super) fn with_build_script(view: &mut RepoView, member: &str, text: &str) -> TestResult {
    let source = path(&format!("crates/{member}/build.rs"))?;
    for metadata in view.rust_metadata.values_mut() {
        for package in &mut metadata.members {
            if package.name.as_str() == member {
                package.targets.push(Target {
                    name: TargetName::new("build-script-build")?,
                    kinds: vec![TargetKind::BuildScript],
                    tests: false,
                    doctest: false,
                    source: Some(source.clone()),
                    requires_features: false,
                    required_features: Vec::new(),
                });
            }
        }
    }
    view.tracked_paths.insert(source.clone());
    view.files.insert(source, text.as_bytes().to_vec());
    Ok(())
}

pub(super) fn outcome(
    source: &str,
    scripts: &[(&str, &str)],
) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    let mut view = view_of(
        &["app", "c"],
        &[
            ("crates/c/src/lib.rs", source),
            ("crates/app/src/lib.rs", ""),
            ("crates/c/src/x.md", ""),
        ],
    )?;
    for (member, text) in scripts {
        with_build_script(&mut view, member, text)?;
    }
    selected(&view, "docs/other.md")
}

pub(super) fn owed(rows: &[(&str, &str)]) -> Vec<(String, String)> {
    rows.iter()
        .map(|(name, reason)| ((*name).to_owned(), (*reason).to_owned()))
        .collect()
}

#[test]
fn an_out_dir_include_is_a_build_output_with_no_input() -> TestResult {
    let mut view = view_of(
        &["app", "tool", "reader", "left"],
        &[
            (
                "crates/reader/src/lib.rs",
                "use std::env;\nuse std::fs;\nuse crate::io::env::Engine;\nuse crate::gate::{env as base_env, policy};\ninclude!(concat!(env!(\"OUT_DIR\"), \"/generated.rs\"));\nconst B: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/blob.bin\"));",
            ),
            (
                "crates/reader/src/util.rs",
                "use crate::helpers::{concat, env, include_str};",
            ),
            ("crates/app/src/lib.rs", ""),
        ],
    )?;
    with_build_script(&mut view, "reader", DECLARES)?;
    assert_eq!(selected(&view, "docs/other.md")?, Vec::new());
    Ok(())
}

#[test]
fn an_out_dir_include_records_no_input_only_when_its_build_script_declares_what_it_reads()
-> TestResult {
    assert_eq!(outcome(OUT, &[("c", DECLARES)])?, Vec::new());
    assert_eq!(
        outcome(
            OUT,
            &[(
                "c",
                "fn main() { println!(\"cargo::rerun-if-changed=../../docs/in.md\"); }"
            )]
        )?,
        Vec::new()
    );
    for (what, scripts) in [
        ("no build script", Vec::new()),
        (
            "a build script that prints nothing",
            vec![("c", "fn main() {}")],
        ),
        (
            "a build script that declares only an environment variable",
            vec![(
                "c",
                "fn main() { println!(\"cargo:rerun-if-env-changed=HOME\"); }",
            )],
        ),
        (
            "a declaring build script in another package",
            vec![("app", DECLARES), ("c", "fn main() {}")],
        ),
    ] {
        assert_eq!(outcome(OUT, &scripts)?, owed(&UNRESOLVED), "{what}");
    }
    Ok(())
}

#[test]
fn a_build_script_that_sets_the_manifest_or_out_dir_makes_its_package_unresolved() -> TestResult {
    let resolved = "const A: &str = include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/src/x.md\"));\nconst B: &str = include_str!(concat!(env!(\"OUT_DIR\"), \"/g.md\"));";
    for line in [
        "println!(\"cargo:rustc-env=CARGO_MANIFEST_DIR=/elsewhere\");",
        "println!(\"cargo::rustc-env=CARGO_MANIFEST_DIR=/elsewhere\");",
        "println!(\"cargo:rustc-env=OUT_DIR=/elsewhere\");",
        "println!(\"cargo::rustc-env=OUT_DIR=/elsewhere\");",
    ] {
        let script =
            format!("fn main() {{ println!(\"cargo:rerun-if-changed=build.rs\"); {line} }}");
        for source in [resolved, ""] {
            assert_eq!(
                outcome(source, &[("c", &script)])?,
                owed(&UNRESOLVED),
                "{line}"
            );
        }
    }
    for line in [
        "println!(\"cargo:rustc-env=GIT_HASH=abc\");",
        "println!(\"cargo:rustc-env=OUT_DIRECTORY=abc\");",
        "println!(\"cargo:rustc-cfg=manifest\");",
        "println!(\"cargo:rustc-env=CARGO_MANIFEST_DIR_HASH=abc\");",
    ] {
        let script =
            format!("fn main() {{ println!(\"cargo:rerun-if-changed=build.rs\"); {line} }}");
        assert_eq!(outcome(resolved, &[("c", &script)])?, Vec::new(), "{line}");
    }
    Ok(())
}

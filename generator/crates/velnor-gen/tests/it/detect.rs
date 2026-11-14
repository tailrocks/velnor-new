// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{DependencyKind, PathGlob, RepoView, TargetKind};
use velnor_rust::{CONFIG_PATH, DetectError, Reason, detect};

use crate::facts::{Shape, TestResult, lib, package, path, repo, root, shape};

#[test]
fn workspaces_and_standalone_packages_each_become_a_root_with_their_members() -> TestResult {
    let cases: Vec<(RepoView, Shape)> = vec![
        (
            repo(
                vec![(
                    "Cargo.toml",
                    vec![
                        lib("a", "crates/a/Cargo.toml")?,
                        lib("b", "crates/b/Cargo.toml")?,
                    ],
                )],
                &[],
            )?,
            vec![root(
                "Cargo.toml",
                &[("a", Some("crates/a")), ("b", Some("crates/b"))],
            )],
        ),
        (
            repo(
                vec![
                    ("Cargo.toml", vec![lib("a", "a/Cargo.toml")?]),
                    ("tools/Cargo.toml", vec![lib("t", "tools/t/Cargo.toml")?]),
                ],
                &[],
            )?,
            vec![
                root("Cargo.toml", &[("a", Some("a"))]),
                root("tools/Cargo.toml", &[("t", Some("tools/t"))]),
            ],
        ),
        (
            repo(
                vec![
                    ("Cargo.toml", vec![lib("outer", "Cargo.toml")?]),
                    (
                        "crates/inner/Cargo.toml",
                        vec![lib("inner", "crates/inner/x/Cargo.toml")?],
                    ),
                ],
                &[],
            )?,
            vec![
                root("Cargo.toml", &[("outer", None)]),
                root(
                    "crates/inner/Cargo.toml",
                    &[("inner", Some("crates/inner/x"))],
                ),
            ],
        ),
        (
            repo(
                vec![("solo/Cargo.toml", vec![lib("solo", "solo/Cargo.toml")?])],
                &[],
            )?,
            vec![root("solo/Cargo.toml", &[("solo", Some("solo"))])],
        ),
    ];
    for (view, expected) in cases {
        assert_eq!(shape(&detect(&view)?), expected);
    }
    Ok(())
}

#[test]
fn path_dependencies_are_edges_between_members_and_between_roots() -> TestResult {
    let helper = package(
        "velnor-ci",
        "ci-helper/crates/velnor-ci/Cargo.toml",
        &[
            (
                "velnor-model",
                DependencyKind::Normal,
                "generator/crates/velnor-model/Cargo.toml",
            ),
            (
                "support",
                DependencyKind::Dev,
                "ci-helper/crates/support/Cargo.toml",
            ),
            (
                "fixture",
                DependencyKind::Normal,
                "fixtures/fixture/Cargo.toml",
            ),
        ],
        &[],
    )?;
    let view = repo(
        vec![
            (
                "ci-helper/Cargo.toml",
                vec![
                    helper,
                    lib("support", "ci-helper/crates/support/Cargo.toml")?,
                ],
            ),
            (
                "generator/Cargo.toml",
                vec![lib(
                    "velnor-model",
                    "generator/crates/velnor-model/Cargo.toml",
                )?],
            ),
        ],
        &[],
    )?;
    let facts = detect(&view)?;
    let edges: Vec<(String, Vec<String>)> = facts
        .rust_roots
        .iter()
        .map(|root| {
            (
                root.manifest.to_string(),
                root.depends_on.iter().map(ToString::to_string).collect(),
            )
        })
        .collect();
    assert_eq!(
        edges,
        vec![
            (
                "ci-helper/Cargo.toml".to_owned(),
                vec!["generator/Cargo.toml".to_owned()]
            ),
            ("generator/Cargo.toml".to_owned(), vec![]),
        ]
    );
    let dependencies: Vec<String> = facts.rust_roots[0]
        .members
        .iter()
        .filter(|member| member.name.as_str() == "velnor-ci")
        .flat_map(|member| &member.dependencies)
        .map(|d| format!("{} {:?} {}", d.name, d.kind, d.manifest))
        .collect();
    assert_eq!(
        dependencies,
        [
            "velnor-model Normal generator/crates/velnor-model/Cargo.toml",
            "support Dev ci-helper/crates/support/Cargo.toml",
            "fixture Normal fixtures/fixture/Cargo.toml",
        ]
    );
    Ok(())
}

#[test]
fn member_targets_are_kept_and_cargos_test_flag_decides_which_members_have_tests() -> TestResult {
    let kept = package(
        "a-kept",
        "a-kept/Cargo.toml",
        &[],
        &[
            ("a-kept", TargetKind::Lib, false),
            ("tool", TargetKind::Bin, false),
            ("derive", TargetKind::ProcMacro, false),
            ("build-script-build", TargetKind::BuildScript, false),
            ("smoke", TargetKind::Test, true),
        ],
    )?;
    let unit = package(
        "b-unit",
        "b-unit/Cargo.toml",
        &[],
        &[("b-unit", TargetKind::Lib, true)],
    )?;
    let none = package(
        "c-none",
        "c-none/Cargo.toml",
        &[],
        &[
            ("c-none", TargetKind::Lib, false),
            ("bench", TargetKind::Bench, false),
        ],
    )?;
    let view = repo(vec![("Cargo.toml", vec![kept, unit, none])], &[])?;
    let facts = detect(&view)?;
    let members = &facts.rust_roots[0].members;
    let kinds: Vec<Vec<TargetKind>> = members[0]
        .targets
        .iter()
        .map(|target| target.kinds.clone())
        .collect();
    assert_eq!(
        kinds,
        vec![
            vec![TargetKind::Lib],
            vec![TargetKind::Bin],
            vec![TargetKind::ProcMacro],
            vec![TargetKind::BuildScript],
            vec![TargetKind::Test],
        ]
    );
    assert_eq!(
        members
            .iter()
            .map(velnor_model::RustMember::has_test_target)
            .collect::<Vec<_>>(),
        vec![true, true, false]
    );
    Ok(())
}

#[test]
fn a_root_without_a_committed_lockfile_is_an_error_unless_it_is_excluded() -> TestResult {
    let mut view = repo(
        vec![
            ("Cargo.toml", vec![lib("a", "a/Cargo.toml")?]),
            ("b/Cargo.toml", vec![lib("b", "b/b/Cargo.toml")?]),
        ],
        &[],
    )?;
    view.tracked_paths.remove(&path("b/Cargo.lock")?);
    assert_eq!(
        detect(&view),
        Err(DetectError::MissingLockfile {
            path: path("b/Cargo.lock")?,
            manifest: path("b/Cargo.toml")?,
        })
    );
    view.rust_metadata.remove(&path("b/Cargo.toml")?);
    view.tracked_paths.remove(&path("b/Cargo.lock")?);
    view.tracked_paths.insert(CONFIG_PATH);
    view.files
        .insert(CONFIG_PATH, b"exclude = [\"b\"]\n".to_vec());
    assert_eq!(
        shape(&detect(&view)?),
        vec![root("Cargo.toml", &[("a", Some("a"))])]
    );
    Ok(())
}

#[test]
fn an_exclude_glob_matches_a_path_or_anything_below_it() -> TestResult {
    let cases = [
        ("fixtures/**", "fixtures/a/Cargo.toml", true),
        ("fixtures/**", "fixtures/Cargo.toml", true),
        ("fixtures/**", "fixture/a/Cargo.toml", false),
        ("fixtures", "fixtures/a/b/Cargo.toml", true),
        ("fixtures", "fixtures2/Cargo.toml", false),
        (
            "crates/**/tests/fixtures/**",
            "crates/a/b/tests/fixtures/x/Cargo.toml",
            true,
        ),
        (
            "crates/**/tests/fixtures/**",
            "crates/a/tests/fixtures/Cargo.toml",
            true,
        ),
        (
            "crates/**/tests/fixtures/**",
            "crates/a/tests/other/Cargo.toml",
            false,
        ),
        ("crates/**/tests", "crates/tests/Cargo.toml", true),
        ("crates/*/tests", "crates/a/tests/x/Cargo.toml", true),
        ("crates/*/tests", "crates/a/b/tests/Cargo.toml", false),
        ("crates/velnor-*", "crates/velnor-runner/Cargo.toml", true),
        ("crates/*-runner", "crates/velnor-core/Cargo.toml", false),
        (
            "**/**/**/**/**/**/**/**/**/**/**/**/**/**/**/**/missing",
            "a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p/q/r/s/t/Cargo.toml",
            false,
        ),
        ("Cargo.toml", "Cargo.toml", true),
        ("Cargo.toml", "a/Cargo.toml", false),
    ];
    for (glob, manifest, expected) in cases {
        assert_eq!(
            PathGlob::new(glob)?.matches(&path(manifest)?),
            expected,
            "{glob} against {manifest}"
        );
    }
    Ok(())
}

#[test]
fn an_exclude_entry_that_matches_no_manifest_is_an_error_naming_the_entry() -> TestResult {
    let mut view = repo(vec![("Cargo.toml", vec![lib("a", "a/Cargo.toml")?])], &[])?;
    view.tracked_paths.insert(CONFIG_PATH);
    view.files
        .insert(CONFIG_PATH, b"exclude = [\"fixtures/**\"]\n".to_vec());
    assert_eq!(
        detect(&view),
        Err(DetectError::Invalid {
            path: CONFIG_PATH,
            reason: Reason::UnmatchedExclude(PathGlob::new("fixtures/**")?),
        })
    );
    Ok(())
}

#[test]
fn an_unknown_config_key_is_an_error_naming_the_key() -> TestResult {
    let mut view = repo(vec![("Cargo.toml", vec![lib("a", "a/Cargo.toml")?])], &[])?;
    view.files.insert(
        CONFIG_PATH,
        b"timeout_minutes = 5\nexcludes = []\n".to_vec(),
    );
    assert_eq!(
        detect(&view),
        Err(DetectError::Invalid {
            path: CONFIG_PATH,
            reason: Reason::UnknownKey("excludes".to_owned()),
        })
    );
    Ok(())
}

#[test]
fn a_repository_without_a_mise_configuration_is_an_error() -> TestResult {
    let mut view = repo(vec![("Cargo.toml", vec![lib("a", "a/Cargo.toml")?])], &[])?;
    view.files.remove(&path("mise.toml")?);
    assert_eq!(
        detect(&view),
        Err(DetectError::MiseRequired {
            path: path("mise.toml")?,
        })
    );
    Ok(())
}

fn detect_with(config: &str) -> Result<velnor_model::RepoFacts, DetectError> {
    let mut view = repo(
        vec![(
            "Cargo.toml",
            vec![lib("a", "a/Cargo.toml").unwrap_or_else(|e| unreachable!("{e}"))],
        )],
        &[],
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    view.tracked_paths.insert(CONFIG_PATH);
    view.files.insert(CONFIG_PATH, config.as_bytes().to_vec());
    detect(&view)
}

fn invalid_value(key: &'static str, value: &str) -> DetectError {
    DetectError::Invalid {
        path: CONFIG_PATH,
        reason: Reason::InvalidValue {
            key,
            value: value.to_owned(),
        },
    }
}

#[test]
fn a_selection_rule_glob_is_segments_ending_in_a_rest_marker_or_an_exact_path() -> TestResult {
    for glob in [
        "docs/**",
        "a/*/b",
        "exact/path.txt",
        "*/x/**",
        "single",
        "a/*",
    ] {
        let facts = detect_with(&format!("non_inputs = [\"{glob}\"]\n"))?;
        assert_eq!(
            facts.config.non_inputs,
            vec![PathGlob::new(glob)?],
            "{glob}"
        );
    }
    for glob in [
        "**",
        "*.md",
        "docs/**/x",
        "a**",
        "docs/*.md",
        "docs/a*",
        "**/docs",
        "a/**/**",
    ] {
        assert_eq!(
            detect_with(&format!("non_inputs = [\"{glob}\"]\n")).map(|_| ()),
            Err(invalid_value("non_inputs", glob)),
            "{glob}"
        );
        assert_eq!(
            detect_with(&format!(
                "[[inputs]]\nglob = \"{glob}\"\npackages = [\"a\"]\n"
            ))
            .map(|_| ()),
            Err(invalid_value("inputs", glob)),
            "{glob}"
        );
    }
    Ok(())
}

#[test]
fn a_non_inputs_glob_that_overlaps_an_inputs_glob_is_an_error_naming_both() -> TestResult {
    let cases = [
        ("docs/**", "docs/api/spec.yml", true),
        ("docs/**", "docs/**", true),
        ("docs/**", "docs2/**", false),
        ("docs/**", "other/docs/x", false),
        ("docs/*/a", "docs/b/**", true),
        ("docs/*/a", "docs/b/c", false),
        ("*/x", "y/*", true),
        ("a/b/**", "a/c/**", false),
        ("a/*", "a/*/b", false),
        ("a/b", "a/b/c", false),
        ("a/b", "a/b", true),
        ("*/*/**", "x/y", true),
    ];
    for (non_input, input, overlap) in cases {
        let result = detect_with(&format!(
            "non_inputs = [\"{non_input}\"]\n[[inputs]]\nglob = \"{input}\"\npackages = [\"a\"]\n"
        ))
        .map(|_| ());
        let expected = if overlap {
            Err(DetectError::Invalid {
                path: CONFIG_PATH,
                reason: Reason::OverlappingRules {
                    non_input: PathGlob::new(non_input)?,
                    input: PathGlob::new(input)?,
                },
            })
        } else {
            Ok(())
        };
        assert_eq!(result, expected, "{non_input} against {input}");
    }
    Ok(())
}

#[test]
fn an_inputs_entry_must_name_packages_of_the_repository() -> TestResult {
    assert_eq!(
        detect_with("[[inputs]]\nglob = \"docs/**\"\npackages = [\"nope\"]\n").map(|_| ()),
        Err(DetectError::Invalid {
            path: CONFIG_PATH,
            reason: Reason::UnknownInputPackage("nope".to_owned()),
        })
    );
    assert_eq!(
        detect_with("[[inputs]]\nglob = \"docs/**\"\npackages = []\n").map(|_| ()),
        Err(invalid_value("inputs.packages", "docs/**"))
    );
    let facts = detect_with("[[inputs]]\nglob = \"docs/**\"\npackages = [\"a\"]\n")?;
    assert_eq!(facts.config.inputs.len(), 1);
    Ok(())
}

#[test]
fn default_ignores_is_on_unless_the_config_turns_it_off() -> TestResult {
    assert!(detect_with("")?.config.default_ignores);
    assert!(
        !detect_with("default_ignores = false\n")?
            .config
            .default_ignores
    );
    assert!(
        detect_with("default_ignores = true\n")?
            .config
            .default_ignores
    );
    Ok(())
}

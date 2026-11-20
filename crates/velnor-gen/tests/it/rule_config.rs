// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::PathGlob;
use velnor_rust::{CONFIG_PATH, DetectError, Reason, detect};

use crate::facts::{TestResult, lib, repo};

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
        ("*/*/**", "x/y", false),
        ("*/*/**", "x/y/z", true),
        ("docs/**", "docs", false),
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

#[test]
fn a_glob_with_a_question_mark_or_a_bracket_is_refused_for_every_rule_key() -> TestResult {
    for glob in ["cfg/h?.pkl", "cfg/h[k].pkl", "cfg/h]k.pkl", "?/x/**"] {
        for config in [
            format!("non_inputs = [\"{glob}\"]\n"),
            format!("[[inputs]]\nglob = \"{glob}\"\npackages = [\"a\"]\n"),
        ] {
            let error = detect_with(&config).err().ok_or(config.clone())?;
            let text = error.to_string();
            assert!(
                text.contains(&format!(
                    "{glob:?} uses ?, [ or ]; only * and ** are supported in a glob"
                )),
                "{config}: {text}"
            );
        }
    }
    Ok(())
}

#[test]
fn a_test_group_path_with_a_question_mark_or_a_bracket_is_refused() -> TestResult {
    for glob in ["cfg/h?.pkl", "cfg/h[k].pkl", "cfg/h]k.pkl"] {
        let config = format!("[[test_groups]]\nname = 'g'\nfilter = 'all()'\npaths = ['{glob}']\n");
        assert!(detect_with(&config).is_err(), "{config}");
    }
    Ok(())
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{Case, aliased_secret, path_matches, segment_matches, selects};

#[test]
fn an_exact_match_tells_apart_names_that_differ_only_in_case() {
    assert!(!segment_matches("JUnit.xml", "junit.xml", Case::Exact));
    assert!(segment_matches("JUnit.xml", "JUnit.xml", Case::Exact));
    assert!(!segment_matches("rep*.XML", "reports.xml", Case::Exact));
    assert!(segment_matches("rep*.xml", "reports.xml", Case::Exact));
    assert!(!path_matches(
        &["reports", "JUnit.xml"],
        &["reports", "junit.xml"],
        Case::Exact
    ));
    assert!(!selects(&["Reports"], &["reports", "a.xml"], Case::Exact));
}

#[test]
fn a_folded_match_ignores_case() {
    assert!(segment_matches("JUnit.xml", "junit.xml", Case::Folded));
    assert!(path_matches(
        &["Reports", "*.XML"],
        &["reports", "a.xml"],
        Case::Folded
    ));
}

#[test]
fn a_wildcard_alias_of_a_secret_is_found_whatever_its_case() {
    assert_eq!(aliased_secret("SECR*").as_deref(), Some("secret"));
    assert_eq!(aliased_secret("Tok*").as_deref(), Some("token"));
}

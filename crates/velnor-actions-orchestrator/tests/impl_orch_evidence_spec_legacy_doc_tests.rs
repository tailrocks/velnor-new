//! Closed function documentation controls for the legacy scanner adapter.

use crate::impl_common::TestResult;

const DOC_COMMENT: &str = "/// Resolve and render standalone task-suite workflow files.";
const DOC_VALUE: &str = " Resolve and render standalone task-suite workflow files.";
const DOC_ATTRIBUTE: &str =
    "#[doc = \" Resolve and render standalone task-suite workflow files.\"]";

fn with_doc_replacement(replacement: &str) -> String {
    super::replace_once(super::ACTUAL_SOURCE, DOC_COMMENT, replacement)
}

fn with_doc_and_extra_attribute(extra: &str) -> String {
    with_doc_replacement(&format!("{DOC_ATTRIBUTE}\n{extra}"))
}

fn accepted(source: &str) -> TestResult {
    let (_fixture, masked) = super::mask(source)?;
    assert_eq!(masked, super::mask_expected_passives(source)?);
    super::assert_three_passive_spans(source, &masked);
    assert!(!super::legacy_fetch_hits(&masked));
    Ok(())
}

#[test]
fn production_doc_comment_is_accepted_without_masking_its_text() -> TestResult {
    accepted(super::ACTUAL_SOURCE)
}

#[test]
fn canonical_doc_name_value_is_accepted_by_the_full_adapter() -> TestResult {
    accepted(&with_doc_replacement(DOC_ATTRIBUTE))
}

#[test]
fn no_doc_minimal_source_is_accepted_independently() -> TestResult {
    accepted(super::MINIMAL_SOURCE)
}

#[test]
fn extra_function_attributes_fail_closed_with_the_canonical_doc_present() -> TestResult {
    for extra in [
        "#[inline]",
        "#[cfg(any())]",
        "#[cfg_attr(any(), inline)]",
        "#[allow(dead_code)]",
        "#[unknown_attribute]",
    ] {
        super::rejected(&with_doc_and_extra_attribute(extra))?;
    }
    Ok(())
}

#[test]
fn noncanonical_doc_attributes_fail_closed() -> TestResult {
    let changed = [
        "#[doc = \" Resolve and render standalone task-suite workflow files. \"]",
        "#[doc = \" Different documentation.\"]",
        "#[doc(hidden)]",
        "#[doc]",
        "#[custom::doc = \" Resolve and render standalone task-suite workflow files.\"]",
        "#[::doc = \" Resolve and render standalone task-suite workflow files.\"]",
        "#[r#doc = \" Resolve and render standalone task-suite workflow files.\"]",
        "#[doc = 1]",
        "#[doc = \" Resolve and render standalone task-suite workflow files.\"suffix]",
        "#[doc = #[allow(dead_code)] \" Resolve and render standalone task-suite workflow files.\"]",
        "#[doc = (\" Resolve and render standalone task-suite workflow files.\")]",
        "#[doc = include_str!(\"unused-doc.txt\")]",
        "#[doc = concat!(\" Resolve and render standalone task-suite workflow files.\")]",
        "//! Resolve and render standalone task-suite workflow files.",
        "#![doc = \" Resolve and render standalone task-suite workflow files.\"]",
    ];
    for replacement in changed {
        super::rejected(&with_doc_replacement(replacement))?;
    }
    let duplicate = format!("{DOC_ATTRIBUTE}\n{DOC_ATTRIBUTE}");
    super::rejected(&with_doc_replacement(&duplicate))?;
    let no_leading_space = format!("#[doc = \"{}\"]", DOC_VALUE.trim_start());
    super::rejected(&with_doc_replacement(&no_leading_space))?;
    Ok(())
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! A pin one release below the first `velnor-ci` that works must not get the
//! rendering. Each case renders with a config and a pin, and checks whether the
//! generator refuses the pin with the matching `*HelperTooOld` error.

use std::error::Error;

use crate::facts::path;
use crate::pipeline::{pin_helper, view, workflow};

const DOCS: &str = "docs = \"post_merge\"\n";
const CHECK_ALONE: &str = "check_alone = \"post_merge\"\n";
const LABEL: &str = "pr_ci = \"label\"\n";

/// The rendering error for `config` pinned at `version`, or an empty string.
fn refusal(config: &str, version: &str) -> Result<String, Box<dyn Error>> {
    let mut repo = view(&["Cargo.toml"], None)?;
    pin_helper(&mut repo, version)?;
    repo.files
        .insert(path(".velnor/rust-ci.toml")?, config.as_bytes().to_vec());
    Ok(match workflow(&repo) {
        Ok(_) => String::new(),
        Err(error) => format!("{error:?}"),
    })
}

#[test]
fn post_merge_docs_are_refused_before_the_helper_defers_them() -> Result<(), Box<dyn Error>> {
    let refused = refusal(DOCS, "0.1.96")?;
    assert!(refused.contains("DocsHelperTooOld"), "{refused}");
    Ok(())
}

#[test]
fn post_merge_docs_render_on_the_first_helper_that_defers_them() -> Result<(), Box<dyn Error>> {
    let refused = refusal(DOCS, "0.1.97")?;
    assert!(!refused.contains("HelperTooOld"), "{refused}");
    Ok(())
}

#[test]
fn post_merge_check_alone_is_refused_before_the_helper_defers_it() -> Result<(), Box<dyn Error>> {
    let refused = refusal(CHECK_ALONE, "0.1.96")?;
    assert!(refused.contains("CheckAloneHelperTooOld"), "{refused}");
    Ok(())
}

#[test]
fn post_merge_check_alone_renders_on_the_first_helper_that_defers_it() -> Result<(), Box<dyn Error>>
{
    let refused = refusal(CHECK_ALONE, "0.1.97")?;
    assert!(!refused.contains("HelperTooOld"), "{refused}");
    Ok(())
}

#[test]
fn pr_ci_label_is_refused_before_the_helper_knows_the_key() -> Result<(), Box<dyn Error>> {
    let refused = refusal(LABEL, "0.1.87")?;
    assert!(refused.contains("PrCiHelperTooOld"), "{refused}");
    Ok(())
}

#[test]
fn pr_ci_label_renders_on_the_first_helper_that_knows_the_key() -> Result<(), Box<dyn Error>> {
    let refused = refusal(LABEL, "0.1.88")?;
    assert!(!refused.contains("HelperTooOld"), "{refused}");
    Ok(())
}

const CACHE_DEBUG: &str = "cache_debug = true\n";
const REQUIRED_CHECK: &str = "required_check = \"Gate\"\n";

#[test]
fn cache_debug_is_refused_before_the_helper_knows_the_key() -> Result<(), Box<dyn Error>> {
    let refused = refusal(CACHE_DEBUG, "0.1.81")?;
    assert!(refused.contains("CacheDebugHelperTooOld"), "{refused}");
    Ok(())
}

#[test]
fn cache_debug_renders_on_the_first_helper_that_knows_the_key() -> Result<(), Box<dyn Error>> {
    let refused = refusal(CACHE_DEBUG, "0.1.82")?;
    assert!(!refused.contains("HelperTooOld"), "{refused}");
    Ok(())
}

#[test]
fn a_required_check_name_is_refused_before_the_helper_reads_it() -> Result<(), Box<dyn Error>> {
    let refused = refusal(REQUIRED_CHECK, "0.1.96")?;
    assert!(refused.contains("RequiredCheckHelperTooOld"), "{refused}");
    Ok(())
}

#[test]
fn a_required_check_name_renders_on_the_first_helper_that_reads_it() -> Result<(), Box<dyn Error>> {
    let refused = refusal(REQUIRED_CHECK, "0.1.97")?;
    assert!(!refused.contains("HelperTooOld"), "{refused}");
    Ok(())
}

const GENERATED_CHECK: &str = "[[lint]]\nkind = 'generated-check'\n";

#[test]
fn generated_check_lint_is_refused_before_the_helper_reads_its_kind() -> Result<(), Box<dyn Error>> {
    let refused = refusal(GENERATED_CHECK, "0.1.106")?;
    assert!(refused.contains("GeneratedCheckHelperTooOld"), "{refused}");
    Ok(())
}

#[test]
fn generated_check_lint_renders_on_the_first_helper_that_reads_its_kind() -> Result<(), Box<dyn Error>>
{
    let refused = refusal(GENERATED_CHECK, "0.1.107")?;
    assert!(!refused.contains("HelperTooOld"), "{refused}");
    Ok(())
}

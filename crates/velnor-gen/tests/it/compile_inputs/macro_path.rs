// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::build_output::{DECLARES, with_build_script};
use super::embed::{selected, view_of};
use crate::facts::TestResult;

const OUT: &str = "concat!(env!(\"OUT_DIR\"), \"/g.txt\")";
const GUIDE: &str = "\"../../../docs/guide.md\"";

fn outcome(source: &str, file: &str) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    let mut view = view_of(
        &["app", "c"],
        &[
            ("crates/c/src/lib.rs", source),
            ("crates/app/src/lib.rs", ""),
            ("docs/guide.md", ""),
        ],
    )?;
    with_build_script(&mut view, "c", DECLARES)?;
    selected(&view, file)
}

fn resolved(source: &str) -> TestResult {
    assert_eq!(outcome(source, "docs/other.md")?, Vec::new(), "{source}");
    Ok(())
}

fn unresolved(source: &str) -> TestResult {
    assert_eq!(
        outcome(source, "docs/other.md")?,
        [("c".to_owned(), "unresolved-include".to_owned())],
        "{source}"
    );
    Ok(())
}

#[test]
fn an_include_macro_named_through_a_path_other_than_std_or_core_is_unresolved() -> TestResult {
    for name in [
        "ext::include_str",
        "ext::include_bytes",
        "ext::include",
        "crate::include_str",
        "a::std::include_str",
        "a::core::include_str",
        "std::io::include_str",
        "::ext::include_str",
    ] {
        unresolved(&format!("const A: &str = {name}!({OUT});"))?;
        unresolved(&format!("const A: &str = {name}!({GUIDE});"))?;
    }
    for name in [
        "std::include_str",
        "core::include_str",
        "::std::include_str",
        "::core::include_bytes",
        "include_str",
    ] {
        resolved(&format!("const A: &str = {name}!({OUT});"))?;
        assert_eq!(
            outcome(
                &format!("const A: &str = {name}!({GUIDE});"),
                "docs/guide.md"
            )?,
            [("c".to_owned(), "input:docs/guide.md".to_owned())],
            "{name}"
        );
    }
    Ok(())
}

#[test]
fn a_macro_use_extern_crate_marks_its_package_unresolved() -> TestResult {
    let include = format!("const A: &str = include_str!({OUT});");
    for item in [
        "#[macro_use]\nextern crate ext;",
        "#[macro_use(include_str)]\nextern crate ext;",
        "#[macro_use] extern crate ext as e;",
        "#[macro_use]\n#[cfg(unix)]\nextern crate ext;",
        "#[cfg(unix)]\n#[macro_use]\nextern crate ext;",
    ] {
        unresolved(&format!("{item}\n{include}"))?;
    }
    for item in [
        "extern crate ext;",
        "#[cfg(unix)]\nextern crate ext;",
        "#[macro_use]\nextern crate std;",
        "#[macro_use]\nextern crate core;",
        "#[macro_use]\nextern crate alloc;",
        "#[macro_use]\nmod helpers;\nextern crate ext;",
        "#[macro_use]\nmod helpers;",
    ] {
        resolved(&format!("{item}\n{include}"))?;
    }
    Ok(())
}

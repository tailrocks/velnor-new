// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::build_output::{DECLARES, OUT, UNRESOLVED, owed, with_build_script};
use super::embed::{selected, view_of};
use crate::facts::TestResult;
use crate::select::path;

const OVERRIDE: &str = "fn helper() { println!(\"cargo:rustc-env=OUT_DIR=/elsewhere\"); }";
const QUIET: &str = "pub fn helper() { println!(\"cargo:rustc-env=GIT_HASH=abc\"); }";

fn outcome(
    script: &str,
    modules: &[(&str, &str)],
    unread: &[&str],
) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    let mut files = vec![
        ("crates/c/src/lib.rs", OUT),
        ("crates/app/src/lib.rs", ""),
        ("crates/c/src/x.md", ""),
    ];
    files.extend_from_slice(modules);
    let mut view = view_of(&["app", "c"], &files)?;
    for file in unread {
        let file = path(file);
        view.files.remove(&file);
        view.tracked_paths.insert(file);
    }
    with_build_script(&mut view, "c", script)?;
    selected(&view, "docs/other.md")
}

fn owing(script: &str, modules: &[(&str, &str)], unread: &[&str]) -> Result<bool, String> {
    outcome(script, modules, unread)
        .map(|found| found == owed(&UNRESOLVED))
        .map_err(|error| error.to_string())
}

#[test]
fn a_build_script_module_that_overrides_the_environment_makes_its_package_unresolved() {
    for (what, prefix, modules) in [
        (
            "a module file",
            "mod helper;",
            vec![("crates/c/helper.rs", OVERRIDE)],
        ),
        (
            "a module directory",
            "mod helper;",
            vec![("crates/c/helper/mod.rs", OVERRIDE)],
        ),
        (
            "a path module",
            "#[path = \"support/h.rs\"]\nmod helper;",
            vec![("crates/c/support/h.rs", OVERRIDE)],
        ),
        (
            "a restricted public path module",
            "#[path = \"support/h.rs\"]\npub(crate) mod helper;",
            vec![("crates/c/support/h.rs", OVERRIDE)],
        ),
        (
            "a public path module",
            "#[path = \"support/h.rs\"]\npub mod helper;",
            vec![("crates/c/support/h.rs", OVERRIDE)],
        ),
        (
            "a path module behind another attribute",
            "#[path = \"support/h.rs\"]\n#[allow(dead_code)]\nmod helper;",
            vec![("crates/c/support/h.rs", OVERRIDE)],
        ),
        (
            "a module of a module",
            "mod a;",
            vec![("crates/c/a.rs", "mod b;"), ("crates/c/a/b.rs", OVERRIDE)],
        ),
        (
            "a module of a module directory",
            "mod a;",
            vec![
                ("crates/c/a/mod.rs", "mod b;"),
                ("crates/c/a/b.rs", OVERRIDE),
            ],
        ),
        (
            "a module of a path module",
            "#[path = \"s/a.rs\"]\nmod a;",
            vec![("crates/c/s/a.rs", "mod b;"), ("crates/c/s/b.rs", OVERRIDE)],
        ),
        (
            "an include",
            "include!(\"gen.rs\");",
            vec![("crates/c/gen.rs", OVERRIDE)],
        ),
    ] {
        let script = format!("{prefix}\n{DECLARES}");
        assert_eq!(owing(&script, &modules, &[]), Ok(true), "{what}");
    }
}

#[test]
fn a_build_script_module_that_cannot_be_followed_makes_its_package_unresolved() {
    for (what, prefix, modules, unread) in [
        ("a missing module", "mod missing;", vec![], vec![]),
        (
            "a missing path module",
            "#[path = \"nowhere.rs\"]\nmod helper;",
            vec![],
            vec![],
        ),
        (
            "a path outside the repository",
            "#[path = \"../../../x.rs\"]\nmod helper;",
            vec![],
            vec![],
        ),
        (
            "a conditional path",
            "#[cfg_attr(unix, path = \"h.rs\")]\nmod helper;",
            vec![("crates/c/helper.rs", QUIET)],
            vec![],
        ),
        (
            "a module inside an inline module",
            "mod outer { mod inner; }",
            vec![("crates/c/inner.rs", QUIET)],
            vec![],
        ),
        (
            "an include that is not a literal",
            "include!(concat!(env!(\"OUT_DIR\"), \"/x.rs\"));",
            vec![],
            vec![],
        ),
        (
            "an unreadable module",
            "mod helper;",
            vec![("crates/c/helper.rs", QUIET)],
            vec!["crates/c/helper.rs"],
        ),
        (
            "an unreadable module of a module",
            "mod a;",
            vec![("crates/c/a.rs", "mod b;"), ("crates/c/a/b.rs", QUIET)],
            vec!["crates/c/a/b.rs"],
        ),
    ] {
        let script = format!("{prefix}\n{DECLARES}");
        assert_eq!(owing(&script, &modules, &unread), Ok(true), "{what}");
    }
}

#[test]
fn a_build_script_module_that_overrides_nothing_leaves_its_package_resolved() -> TestResult {
    for (what, prefix, modules) in [
        (
            "a module that prints nothing",
            "mod helper;",
            vec![("crates/c/helper.rs", "pub fn helper() {}")],
        ),
        (
            "a module directory",
            "mod helper;",
            vec![("crates/c/helper/mod.rs", QUIET)],
        ),
        (
            "public path modules",
            "#[path = \"support/a.rs\"]\npub mod a;\n#[path = \"support/b.rs\"]\npub(crate) mod b;",
            vec![
                ("crates/c/support/a.rs", QUIET),
                ("crates/c/support/b.rs", QUIET),
            ],
        ),
        (
            "a path module behind another attribute",
            "#[path = \"support/a.rs\"]\n#[allow(dead_code)]\nmod a;",
            vec![("crates/c/support/a.rs", QUIET)],
        ),
        (
            "a module that sets another variable",
            "mod helper;",
            vec![("crates/c/helper.rs", QUIET)],
        ),
        (
            "path modules",
            "#[path = \"support/a.rs\"]\nmod a;\n#[path = \"support/b.rs\"]\nmod b;",
            vec![
                ("crates/c/support/a.rs", QUIET),
                ("crates/c/support/b.rs", "pub fn b() {}"),
            ],
        ),
        (
            "a module of a module",
            "mod a;",
            vec![("crates/c/a.rs", "mod b;"), ("crates/c/a/b.rs", QUIET)],
        ),
        (
            "an include of a quiet file",
            "include!(\"gen.rs\");",
            vec![("crates/c/gen.rs", QUIET)],
        ),
        (
            "a module that is the build script itself",
            "#[path = \"build.rs\"]\nmod again;",
            vec![],
        ),
        ("an inline module", "mod inline { pub fn f() {} }", vec![]),
    ] {
        let script = format!("{prefix}\n{DECLARES}");
        assert_eq!(outcome(&script, &modules, &[])?, Vec::new(), "{what}");
    }
    Ok(())
}

#[test]
fn a_declaration_in_a_build_script_module_does_not_count() -> TestResult {
    let declaring = "pub fn main() { println!(\"cargo:rerun-if-changed=build.rs\"); }";
    assert_eq!(
        owing(
            "mod helper;\nfn main() {}",
            &[("crates/c/helper.rs", declaring)],
            &[]
        ),
        Ok(true)
    );
    assert_eq!(
        outcome(
            &format!("mod helper;\n{DECLARES}"),
            &[("crates/c/helper.rs", "pub fn helper() {}")],
            &[]
        )?,
        Vec::new()
    );
    Ok(())
}

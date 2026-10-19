//! Affected selection's read scan, end to end: packages whose sources read
//! outside the paths they claim run until those inputs are declared.
//!
//! Same ten-crate fixture as `impl_select_affected_jobs`; see
//! `impl_select_fail_safe` for the shared helpers.

use std::path::Path;

use crate::impl_common::TestResult;
use crate::impl_select_affected_jobs::{assert_runs, put};
use crate::impl_select_fail_safe::{CORE_CLOSURE, has_warning, plan_with};

#[test]
fn undeclared_include_into_a_sibling_runs_the_reader() -> TestResult {
    let plan = plan_with(
        "",
        |root| {
            put(root, "libs/util/data.txt", "a\n")?;
            put(
                root,
                "apps/a7/src/lib.rs",
                "pub const D: &str = include_str!(\"../../../libs/util/data.txt\");\n",
            )
        },
        |root| put(root, "libs/util/data.txt", "b\n"),
    )?;
    assert_runs(&plan, &["a2", "a7", "util"]);
    assert!(
        has_warning(
            &plan,
            "undeclared_reads:a7:apps/a7/src/lib.rs:reads:libs/util/data.txt"
        ),
        "{:?}",
        plan.warnings
    );
    Ok(())
}

#[test]
fn declared_include_is_covered_and_narrows_again() -> TestResult {
    let rule = "\n[[stacks.rust.inputs]]\nglobs = [\"libs/util/data.txt\"]\npackages = [\"a7\"]\n";
    let setup = |root: &Path| {
        put(root, "libs/util/data.txt", "a\n")?;
        put(
            root,
            "apps/a7/src/lib.rs",
            "pub const D: &str = include_str!(\"../../../libs/util/data.txt\");\n",
        )
    };
    let plan = plan_with(rule, setup, |root| {
        put(root, "apps/a1/src/lib.rs", "pub fn g() {}\n")
    })?;
    assert_runs(&plan, &["a1"]);
    let plan = plan_with(rule, setup, |root| put(root, "libs/util/data.txt", "b\n"))?;
    assert_runs(&plan, &["a2", "a7", "util"]);
    Ok(())
}

#[test]
fn manifest_dir_read_of_a_deleted_sibling_fixture_runs_the_reader() -> TestResult {
    let plan = plan_with(
        "",
        |root| {
            put(root, "libs/util/fixtures/x.json", "{}\n")?;
            put(
                root,
                "apps/a7/tests/t.rs",
                "#[test] fn t(){ std::fs::read(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../../libs/util/fixtures/x.json\")).unwrap(); }\n",
            )
        },
        |root| {
            Ok(std::fs::remove_file(
                root.join("libs/util/fixtures/x.json"),
            )?)
        },
    )?;
    assert_runs(&plan, &["a2", "a7", "util"]);
    Ok(())
}

#[test]
fn unbounded_manifest_dir_reads_run_until_inputs_are_declared() -> TestResult {
    let reader = |root: &Path| {
        put(
            root,
            "apps/a7/tests/t.rs",
            "#[test] fn t(){ let _ = std::path::Path::new(env!(\"CARGO_MANIFEST_DIR\")).join(\"../..\"); }\n",
        )
    };
    let edit = |root: &Path| put(root, "apps/a1/src/lib.rs", "pub fn g() {}\n");
    let plan = plan_with("", reader, edit)?;
    assert_runs(&plan, &["a1", "a7"]);
    assert!(
        has_warning(
            &plan,
            "undeclared_reads:a7:apps/a7/tests/t.rs:manifest_dir_escape"
        ),
        "{:?}",
        plan.warnings
    );
    // An input rule alone is not a statement that the inputs are complete.
    let partial = "\n[[stacks.rust.inputs]]\nglobs = [\"libs/util/**\"]\npackages = [\"a7\"]\n";
    let plan = plan_with(partial, reader, edit)?;
    assert_runs(&plan, &["a1", "a7"]);
    let complete = "\n[[stacks.rust.inputs]]\nglobs = [\"libs/util/**\"]\npackages = [\"a7\"]\ncomplete = true\n";
    let plan = plan_with(complete, reader, edit)?;
    assert_runs(&plan, &["a1"]);
    let plan = plan_with(complete, reader, |root| {
        put(root, "libs/util/src/lib.rs", "pub fn g() {}\n")
    })?;
    assert_runs(&plan, &["a2", "a7", "util"]);
    Ok(())
}

#[test]
fn reads_of_unowned_paths_never_force() -> TestResult {
    let plan = plan_with(
        "",
        |root| {
            put(
                root,
                "apps/a7/src/lib.rs",
                "pub const D: &str = include_str!(\"../../../docs/notes.md\");\n",
            )
        },
        |root| put(root, "apps/a1/src/lib.rs", "pub fn g() {}\n"),
    )?;
    // A change to `docs/notes.md` is unowned and runs everything already.
    assert_runs(&plan, &["a1"]);
    Ok(())
}

#[test]
fn directory_reads_above_packages_force_the_reader() -> TestResult {
    let plan = plan_with(
        "",
        |root| {
            put(
                root,
                "apps/a7/tests/t.rs",
                "#[test] fn t(){ let _ = std::fs::read_dir(\"../..\"); }\n",
            )
        },
        |root| put(root, "apps/a1/src/lib.rs", "pub fn g() {}\n"),
    )?;
    assert_runs(&plan, &["a1", "a7"]);
    Ok(())
}

#[test]
fn manifest_dir_anchor_and_escape_in_different_files_force_the_package() -> TestResult {
    let plan = plan_with(
        "",
        |root| {
            put(
                root,
                "apps/a7/tests/common/mod.rs",
                "pub fn dir() -> std::path::PathBuf { std::path::PathBuf::from(env!(\"CARGO_MANIFEST_DIR\")) }\n",
            )?;
            put(
                root,
                "apps/a7/tests/t.rs",
                "mod common;\n#[test] fn t(){ let _ = std::fs::read(common::dir().join(\"../../libs/util/x.json\")); }\n",
            )
        },
        |root| put(root, "apps/a1/src/lib.rs", "pub fn g() {}\n"),
    )?;
    assert_runs(&plan, &["a1", "a7"]);
    Ok(())
}

#[test]
fn reads_inside_included_unowned_support_files_count_for_the_includer() -> TestResult {
    let plan = plan_with(
        "",
        |root| {
            put(root, "libs/util/data.txt", "a\n")?;
            put(
                root,
                "shared/support.rs",
                "pub const D: &str = include_str!(\"../libs/util/data.txt\");\n",
            )?;
            put(
                root,
                "apps/a7/tests/t.rs",
                "#[path = \"../../../shared/support.rs\"]\nmod support;\n",
            )
        },
        |root| put(root, "libs/util/data.txt", "b\n"),
    )?;
    assert_runs(&plan, &["a2", "a7", "util"]);
    Ok(())
}

#[test]
fn reads_inside_non_rust_include_fragments_count_for_the_includer() -> TestResult {
    let plan = plan_with(
        "",
        |root| {
            put(root, "libs/util/data.txt", "a\n")?;
            put(
                root,
                "shared/gen.rs.in",
                "pub const D: &str = include_str!(\"../libs/util/data.txt\");\n",
            )?;
            put(
                root,
                "apps/a7/src/lib.rs",
                "include!(\"../../../shared/gen.rs.in\");\n",
            )
        },
        |root| put(root, "libs/util/data.txt", "b\n"),
    )?;
    assert_runs(&plan, &["a2", "a7", "util"]);
    Ok(())
}

#[test]
fn reads_inside_a_declared_module_of_another_package_count_for_the_includer() -> TestResult {
    // a7 declares util's module as its input, so the include itself is
    // covered; the module's own read must still count for a7.
    let rule =
        "\n[[stacks.rust.inputs]]\nglobs = [\"libs/util/src/shared.rs\"]\npackages = [\"a7\"]\n";
    let plan = plan_with(
        rule,
        |root| {
            put(root, "libs/core/data.txt", "a\n")?;
            put(
                root,
                "libs/util/src/shared.rs",
                "pub const D: &str = include_str!(\"../../core/data.txt\");\n",
            )?;
            put(
                root,
                "apps/a7/src/lib.rs",
                "#[path = \"../../../libs/util/src/shared.rs\"]\nmod shared;\n",
            )
        },
        |root| put(root, "libs/core/data.txt", "b\n"),
    )?;
    let mut expected = CORE_CLOSURE.to_vec();
    expected.extend(["a2", "a7", "util"]);
    assert_runs(&plan, &expected);
    Ok(())
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::io_plan_embedded::{NOTHING, embedded_repo, explain};

const OUT_DIR_INCLUDE: &str = "include!(concat!(env!(\"OUT_DIR\"), \"/gen.rs\"));\n";

fn planned(name: &str, include: &str, other: Option<(&str, &str)>, changed: &str) -> String {
    let repo = embedded_repo(name, include);
    if let Some((path, text)) = other {
        repo.write(path, text);
    }
    explain(&repo, changed)
}

fn reads_any_file(name: &str, include: &str, other: Option<(&str, &str)>) {
    let found = planned(name, include, other, "notes/x.md");
    assert!(found.contains("b input:notes/x.md"), "{name}: {found}");
}

#[test]
fn a_crate_that_defines_its_own_concat_or_env_reads_any_file_for_a_build_output_include() {
    for (at, shadow) in [
        "macro_rules! concat { ($($t:tt)*) => { \"../notes/x.md\" }; }\n",
        "macro_rules! env { ($($t:tt)*) => { \"../notes\" }; }\n",
        "macro_rules! include { ($($t:tt)*) => {}; }\n",
        "use helpers::concat;\n",
        "use helpers::env as env;\n",
        "use helpers::{other, concat};\n",
        "use helpers::include_bytes;\n",
        "use helpers::include_str as include_str;\n",
        "use helpers::something as include;\n",
    ]
    .into_iter()
    .enumerate()
    {
        let include = format!("{shadow}{OUT_DIR_INCLUDE}");
        reads_any_file(&format!("shadow-same-{at}"), &include, None);
    }
}

#[test]
fn a_shadowing_macro_in_another_file_of_the_crate_reads_any_file() {
    for (at, (path, text)) in [
        (
            "b/src/macros.rs",
            "macro_rules! env { ($($t:tt)*) => { \"../notes\" }; }\n",
        ),
        ("b/src/uses.rs", "use helpers::include_bytes;\n"),
        ("b/tests/it.rs", "use helpers::concat;\n"),
    ]
    .into_iter()
    .enumerate()
    {
        reads_any_file(
            &format!("shadow-other-{at}"),
            OUT_DIR_INCLUDE,
            Some((path, text)),
        );
    }
}

#[test]
fn a_glob_import_in_the_crate_reads_any_file_for_a_build_output_include() {
    for (at, text) in ["use helpers::*;\n", "use helpers::{a, nested::*};\n"]
        .into_iter()
        .enumerate()
    {
        reads_any_file(
            &format!("shadow-glob-{at}"),
            OUT_DIR_INCLUDE,
            Some(("b/src/uses.rs", text)),
        );
    }
}

#[test]
fn a_macro_use_attribute_in_the_crate_reads_any_file_for_a_build_output_include() {
    for (at, text) in [
        "#[macro_use]\nextern crate helpers;\n",
        "#[macro_use]\nmod macros;\n",
        "#![macro_use]\n",
    ]
    .into_iter()
    .enumerate()
    {
        reads_any_file(
            &format!("shadow-macro-use-{at}"),
            OUT_DIR_INCLUDE,
            Some(("b/src/uses.rs", text)),
        );
    }
}

#[test]
fn a_standard_library_import_of_env_does_not_shadow_a_build_output_include() {
    for (at, text) in [
        "use std::env;\n",
        "use std::{env, concat};\n",
        "use ::core::concat as concat;\n",
    ]
    .into_iter()
    .enumerate()
    {
        let found = planned(
            &format!("shadow-std-{at}"),
            OUT_DIR_INCLUDE,
            Some(("b/src/uses.rs", text)),
            "notes/x.md",
        );
        assert_eq!(found, NOTHING, "{text}");
    }
}

#[test]
fn an_import_that_only_passes_through_a_module_named_env_does_not_shadow() {
    let include = format!(
        "use velnor_local::io::env::{{Engine, Environment}};\n{OUT_DIR_INCLUDE}include_str!(\"../../shared/h.txt\");\n"
    );
    assert_eq!(
        planned("shadow-path-a", &include, None, "notes/x.md"),
        NOTHING
    );
    assert_eq!(
        planned("shadow-path-b", &include, None, "shared/h.txt"),
        "Cargo.toml: some\n  b input:shared/h.txt\ntools/Cargo.toml: none\n"
    );
}

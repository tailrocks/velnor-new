// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::symlink;

use crate::io::Repo;
use crate::io_plan::{plan_command, plan_repo, selected};

const NOTHING: &str = "Cargo.toml: none\ntools/Cargo.toml: none\n";

fn embedded_repo(name: &str, include: &str) -> Repo {
    let repo = plan_repo(name);
    repo.write_all(&[
        (".velnor/rust-ci.toml", "non_inputs = [\"notes/**\"]\n"),
        ("b/src/lib.rs", include),
        ("notes/x.md", "# notes\n"),
        ("shared/h.txt", "header\n"),
    ]);
    repo
}

fn explained(include: &str, changed: &str) -> String {
    let repo = embedded_repo(&format!("embedded-{}", changed.replace('/', "-")), include);
    explain(&repo, changed)
}

fn explain(repo: &Repo, changed: &str) -> String {
    let output = plan_command(repo)
        .args(["plan", "--explain", "--changed", changed])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    selected(&output)
        .lines()
        .filter(|line| !line.starts_with("check does less: "))
        .flat_map(|line| [line, "\n"])
        .collect()
}

#[test]
fn a_build_output_include_reads_no_repository_file() {
    let include = "include!(concat!(env!(\"OUT_DIR\"), \"/gen.rs\"));\n";
    assert_eq!(explained(include, "notes/x.md"), NOTHING);
}

#[test]
fn a_build_output_include_that_climbs_out_of_the_output_folder_can_read_anything() {
    let include = "include_str!(concat!(env!(\"OUT_DIR\"), \"/../../../notes/x.md\"));\n";
    let planned = explained(include, "notes/x.md");
    assert!(planned.contains("b input:notes/x.md"), "{planned}");
}

#[test]
fn a_manifest_dir_include_reads_only_the_file_it_names() {
    let include = "include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../shared/h.txt\"));\n";
    assert_eq!(explained(include, "notes/x.md"), NOTHING);
    assert_eq!(
        explained(include, "shared/h.txt"),
        "Cargo.toml: some\n  b input:shared/h.txt\ntools/Cargo.toml: none\n"
    );
}

#[test]
fn a_manifest_dir_include_split_across_literals_can_read_anything() {
    let include =
        "include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/\", \"../notes/x.md\"));\n";
    let planned = explained(include, "notes/x.md");
    assert!(planned.contains("b input:notes/x.md"), "{planned}");
}

#[test]
fn a_build_output_include_split_across_literals_can_read_anything() {
    let include = "include_str!(concat!(env!(\"OUT_DIR\"), \"/\", \"../../../notes/x.md\"));\n";
    let planned = explained(include, "notes/x.md");
    assert!(planned.contains("b input:notes/x.md"), "{planned}");
}

fn assert_slashless_tail_reads_anything(variable: &str, tail: &str) {
    let include = format!("include_str!(concat!(env!(\"{variable}\"), \"{tail}\"));\n");
    let planned = explained(&include, "notes/x.md");
    assert!(planned.contains("b input:notes/x.md"), "{planned}");
}

#[test]
fn a_manifest_dir_include_tail_without_a_leading_slash_can_read_anything() {
    assert_slashless_tail_reads_anything("CARGO_MANIFEST_DIR", "shared/h.txt");
}

#[test]
fn a_build_output_include_tail_without_a_leading_slash_can_read_anything() {
    assert_slashless_tail_reads_anything("OUT_DIR", "gen.rs");
}

#[test]
fn a_manifest_dir_include_with_a_trailing_comma_still_reads_only_the_file_it_names() {
    let include = "include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../shared/h.txt\",));\n";
    assert_eq!(explained(include, "notes/x.md"), NOTHING);
}

fn linked(name: &str, include: &str, link: &str, target: &str) -> String {
    let repo = embedded_repo(name, include);
    symlink(target, repo.root().join(link)).unwrap_or_else(|e| unreachable!("{e}"));
    repo.git(&["add", "--all"]);
    explain(&repo, "notes/x.md")
}

#[test]
fn a_manifest_dir_include_through_a_tracked_symlink_can_read_anything() {
    let include = "include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/data.txt\"));\n";
    let planned = linked(
        "embedded-link-manifest",
        include,
        "b/data.txt",
        "../notes/x.md",
    );
    assert!(planned.contains("b input:notes/x.md"), "{planned}");
}

#[test]
fn a_relative_include_below_a_tracked_symlinked_folder_can_read_anything() {
    let planned = linked(
        "embedded-link-relative",
        "include_str!(\"inc/x.md\");\n",
        "b/src/inc",
        "../../notes",
    );
    assert!(planned.contains("b input:notes/x.md"), "{planned}");
}

#[test]
fn a_manifest_dir_include_that_climbs_out_of_a_tracked_symlink_can_read_anything() {
    let include = "include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/link/../x.md\"));\n";
    let planned = linked("embedded-climb-manifest", include, "b/link", "../notes");
    assert!(planned.contains("b input:notes/x.md"), "{planned}");
}

#[test]
fn a_relative_include_that_climbs_out_of_a_tracked_symlink_can_read_anything() {
    let planned = linked(
        "embedded-climb-relative",
        "include_str!(\"inc/../x.md\");\n",
        "b/src/inc",
        "../../notes",
    );
    assert!(planned.contains("b input:notes/x.md"), "{planned}");
}

#[test]
fn a_crate_that_defines_its_own_concat_or_env_reads_any_file_for_a_build_output_include() {
    for shadow in [
        "macro_rules! concat { ($($t:tt)*) => { \"../notes/x.md\" }; }\n",
        "macro_rules! env { ($($t:tt)*) => { \"../notes\" }; }\n",
        "use helpers::concat;\n",
        "use helpers::env as env;\n",
    ] {
        let include = format!("{shadow}include!(concat!(env!(\"OUT_DIR\"), \"/gen.rs\"));\n");
        let planned = explained(&include, "notes/x.md");
        assert!(
            planned.contains("b input:notes/x.md"),
            "{shadow}: {planned}"
        );
    }
}

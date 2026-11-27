// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::io::Repo;
use crate::io_plan::{plan_command, plan_repo, selected};

const NOTHING: &str = "Cargo.toml: none\ntools/Cargo.toml: none\n";

fn explained(include: &str, changed: &str) -> String {
    let repo = plan_repo(&format!("embedded-{}", changed.replace('/', "-")));
    repo.write_all(&[
        (".velnor/rust-ci.toml", "non_inputs = [\"notes/**\"]\n"),
        ("b/src/lib.rs", include),
        ("notes/x.md", "# notes\n"),
        ("shared/h.txt", "header\n"),
    ]);
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

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_ci::inputs::{Tracked, TrackedFile, parse_inputs, parse_ls_files};

#[test]
fn only_regular_files_at_stage_zero_with_a_writable_path_are_tracked() {
    let blob = "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391";
    let records = [
        format!("100644 {blob} 0\tsrc/lib.rs"),
        format!("100755 {blob} 0\tscripts/run me"),
        format!("120000 {blob} 0\tlink"),
        format!("160000 {blob} 0\tvendor/sub"),
        format!("100644 {blob} 1\tconflicted.rs"),
        format!("100644 {blob} 0\ttab\there.rs"),
        format!("100644 {blob} 0\tline\nbreak.rs"),
    ];
    let staged = records.join("\0") + "\0";
    let tracked = parse_ls_files(staged.as_bytes());
    let file = |path: &str, mode: &str| TrackedFile {
        path: path.to_owned(),
        blob: blob.to_owned(),
        mode: mode.to_owned(),
    };
    assert_eq!(
        tracked,
        Tracked {
            files: vec![
                file("src/lib.rs", "100644"),
                file("scripts/run me", "100755")
            ],
            absent: vec![],
            unsupported: 2,
        }
    );
}

#[test]
fn a_corrupt_inputs_file_is_an_error_that_names_the_line() {
    let text =
        "src/lib.rs\tabc\t1700000000000000000\t100644\nsrc/main.rs\tdef\tyesterday\t100644\n";
    let error = parse_inputs(text).map(|inputs| inputs.len());
    assert_eq!(
        error.map_err(|e| e.to_string()),
        Err(
            "target/.velnor/inputs.tsv line 2 is not `path<TAB>blob<TAB>mtime_ns<TAB>mode`: `src/main.rs\tdef\tyesterday\t100644`"
                .to_owned()
        )
    );
}

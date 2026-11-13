// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_ci::inputs::{TrackedFile, parse_inputs, parse_ls_files};

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
    let output = records.join("\0") + "\0";
    let tracked = parse_ls_files(output.as_bytes());
    let file = |path: &str| TrackedFile {
        path: path.to_owned(),
        blob: blob.to_owned(),
    };
    assert_eq!(tracked, vec![file("src/lib.rs"), file("scripts/run me")]);
}

#[test]
fn a_corrupt_inputs_file_is_an_error_that_names_the_line() {
    let text = "src/lib.rs\tabc\t1700000000000000000\nsrc/main.rs\tdef\tyesterday\n";
    let error = parse_inputs(text).map(|inputs| inputs.len());
    assert_eq!(
        error.map_err(|e| e.to_string()),
        Err(
            "target/.velnor/inputs.tsv line 2 is not `path<TAB>blob<TAB>mtime_ns`: `src/main.rs\tdef\tyesterday`"
                .to_owned()
        )
    );
}

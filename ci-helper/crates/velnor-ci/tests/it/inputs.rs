// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_ci::inputs::{Tracked, TrackedFile, parse_inputs, parse_ls_files};

#[test]
fn regular_files_and_symlinks_at_stage_zero_with_a_writable_path_are_tracked() {
    let blob = "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391";
    let records = [
        format!("100644 {blob} 0\tsrc/lib.rs"),
        format!("100755 {blob} 0\tscripts/run me"),
        format!("120000 {blob} 0\tlink"),
        format!("120000 {blob} 0\tlink\twith tab"),
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
            links: vec![file("link", "120000")],
            absent: vec![],
            unsupported: 2,
            unverifiable_links: 1,
            submodules: 1,
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

#[test]
fn breaking_ties_never_lands_on_a_time_another_file_already_has() {
    use velnor_ci::ties::{TIE_STEP_NS, break_ties};
    let t = 5_000_000;
    let files: Vec<(String, u64)> = vec![
        ("a".into(), t),
        ("b".into(), t),
        ("c".into(), t - TIE_STEP_NS),
        ("d".into(), t),
    ];
    let moved = break_ties(&files);
    let mut times: Vec<u64> = files
        .iter()
        .filter(|(path, _)| path == "a" || path == "c")
        .map(|(_, time)| *time)
        .chain(moved.iter().map(|(_, time)| *time))
        .collect();
    times.sort_unstable();
    times.dedup();
    assert_eq!(times.len(), 4);
    assert_eq!(
        moved
            .iter()
            .map(|(path, _)| path.as_str())
            .collect::<Vec<_>>(),
        ["b", "d"]
    );
}

#[test]
fn every_time_breaking_ties_moves_is_strictly_earlier_than_the_original() {
    use std::collections::BTreeMap;
    use velnor_ci::ties::break_ties;
    let files: Vec<(String, u64)> = (0..6)
        .map(|n| (format!("f{n}"), if n < 4 { 9_000_000 } else { 8_999_000 }))
        .collect();
    let original: BTreeMap<&str, u64> = files.iter().map(|(p, t)| (p.as_str(), *t)).collect();
    let moved = break_ties(&files);
    assert_eq!(moved.len(), 4);
    for (path, time) in &moved {
        assert!(*time < original[path.as_str()], "{path} moved up to {time}");
    }
}

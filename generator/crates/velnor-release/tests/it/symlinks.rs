// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::ReleaseFiles;

use crate::fixture::{files, refusal};

fn linked(files: &mut ReleaseFiles, path: &str) {
    files.symlinks.insert(path.to_owned());
}

#[test]
fn the_refusal_names_the_symlink() {
    let mut files = files();
    linked(&mut files, ".cargo");
    let text = refusal(&files).map(|error| error.to_string());
    assert!(
        text.as_deref()
            .is_some_and(|text| text.starts_with(".cargo: ")),
        "{text:?}"
    );
}

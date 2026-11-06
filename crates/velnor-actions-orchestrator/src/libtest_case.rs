// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(crate) const fn name(path: &'static str) -> &'static str {
    let bytes = path.as_bytes();
    let mut index = 0;
    while index + 1 < bytes.len() {
        if bytes[index] == b':' && bytes[index + 1] == b':' {
            return path.split_at(index + 2).1;
        }
        index += 1;
    }
    path
}

pub(crate) fn parent_name(path: &'static str, test: &str) -> String {
    let parent = path.rsplit_once("::").map_or(path, |(parent, _)| parent);
    format!("{}::{test}", name(parent))
}

pub(crate) fn assert_one_passed(output: &std::process::Output) {
    assert!(
        output.status.success()
            && String::from_utf8_lossy(&output.stdout)
                .contains("test result: ok. 1 passed; 0 failed;"),
        "child status: {}\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

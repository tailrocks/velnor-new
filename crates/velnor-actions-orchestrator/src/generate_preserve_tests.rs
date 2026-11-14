// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use super::generator_owned;

#[test]
fn the_workflows_namespace_is_generator_owned_except_the_release_workflow_of_the_rust_tools()
-> Result<(), Box<dyn std::error::Error>> {
    let source = tempfile::NamedTempFile::new()?;
    std::fs::write(source.path(), "name: Plain\n")?;
    let metadata = std::fs::metadata(source.path())?;
    let cases = [
        ("workflows", false),
        ("workflows/ci.yml", true),
        ("workflows/manual.yml", true),
        ("workflows/release-rust-ci-tools.yml", false),
        ("workflows/release-rust-ci-tools.yml.bak", true),
        ("workflows/nested/release-rust-ci-tools.yml", true),
        ("release-rust-ci-tools.yml", false),
    ];
    for (path, owned) in cases {
        assert_eq!(
            generator_owned(Path::new(path), source.path(), &metadata)?,
            owned,
            "{path}"
        );
    }
    Ok(())
}

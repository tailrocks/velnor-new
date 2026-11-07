// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::support::Fixture;
use std::{error::Error, fs};

pub(super) fn assert_rejected(
    fixture: &Fixture,
    output: &std::process::Output,
    expected: &str,
) -> Result<(), Box<dyn Error>> {
    assert!(!output.status.success(), "accepted invalid CI evidence");
    let diagnostic = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        diagnostic.contains(expected),
        "expected {expected}; got {diagnostic}"
    );
    assert!(fixture.last_fixture_error()?.is_none());
    if fixture.receipt.exists() {
        assert_eq!(
            super::super::support::read_pending(&fixture.receipt)?["expected_digest"],
            fixture.expected_digest
        );
    }
    Ok(())
}

pub(super) fn assert_materialized_layout(fixture: &Fixture) -> Result<(), Box<dyn Error>> {
    let mut directories = fs::read_dir(&fixture.scratch)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|path| path.is_dir());
    let publication = directories
        .next()
        .ok_or("publication directory missing at manifest lookup")?;
    if directories.next().is_some() || !publication.join("oci/oci-layout").is_file() {
        return Err("manifest lookup did not follow OCI materialization".into());
    }
    Ok(())
}

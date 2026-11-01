// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{fs, os::unix::fs::symlink};

use velnor_actions_mise::catalog::{
    COSIGN_DARWIN_AMD64_SHA256, COSIGN_DARWIN_ARM64_SHA256, COSIGN_LINUX_AMD64_SHA256,
    COSIGN_LINUX_ARM64_SHA256,
};

use super::fixture::{
    CATALOG, Fixture, INVENTORY, POLICY, RUNNER_SOURCE, TestResult, set_first_line,
};

const COSIGN_ARTIFACTS: [(&str, &str, &str); 4] = [
    ("darwin-arm64", "DARWIN_ARM64", COSIGN_DARWIN_ARM64_SHA256),
    ("darwin-amd64", "DARWIN_AMD64", COSIGN_DARWIN_AMD64_SHA256),
    ("linux-arm64", "LINUX_ARM64", COSIGN_LINUX_ARM64_SHA256),
    ("linux-amd64", "LINUX_AMD64", COSIGN_LINUX_AMD64_SHA256),
];

#[test]
fn pin_mirror_rejects_catalog_drift_for_every_cosign_artifact() -> TestResult {
    for (platform, constant, digest) in COSIGN_ARTIFACTS {
        let fixture = Fixture::new()?;
        fixture.rewrite(CATALOG, |text| text.replacen(digest, &"ab".repeat(32), 1))?;
        let outcome = fixture.run(None)?;
        assert!(!outcome.success, "{platform} digest drift accepted");
        let subject = format!("tool cosign {platform} release digest");
        assert!(
            outcome.row("local-pin", "fail", &subject).is_some(),
            "{subject} not named: {:?}",
            outcome.rows
        );

        let fixture = Fixture::new()?;
        let prefix = format!("pub const COSIGN_{constant}_BYTES: usize = ");
        fixture.rewrite(CATALOG, |text| {
            set_first_line(&text, &prefix, &format!("{prefix}1;"))
        })?;
        let outcome = fixture.run(None)?;
        assert!(!outcome.success, "{platform} size drift accepted");
        let subject = format!("tool cosign {platform} release size");
        assert!(
            outcome.row("local-pin", "fail", &subject).is_some(),
            "{subject} not named: {:?}",
            outcome.rows
        );
    }
    Ok(())
}

#[test]
fn pin_mirror_rejects_supported_runners_that_differ_from_the_compiled_catalog() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.rewrite(POLICY, |text| text.replacen(", \"ubuntu-22.04\"", "", 1))?;
    fixture.edit_inventory(|inventory| {
        if let Some(labels) = inventory["runner"]["supported"].as_array_mut() {
            labels.retain(|label| label != "ubuntu-22.04");
        }
    })?;
    let outcome = fixture.run(None)?;
    assert!(!outcome.success, "mirrors dropped a compiled label");
    assert!(
        outcome.row("local-pin", "fail", "runner catalog").is_some(),
        "catalog drift not named: {:?}",
        outcome.rows
    );

    let fixture = Fixture::new()?;
    fixture.rewrite(RUNNER_SOURCE, |text| {
        text.replacen("[&str; 6] = [", "[&str; 7] = [\n    \"ubuntu-28.04\",", 1)
    })?;
    let outcome = fixture.run(None)?;
    assert!(!outcome.success, "compiled label missing from mirrors");
    assert!(
        outcome.row("local-pin", "fail", "runner catalog").is_some(),
        "catalog drift not named: {:?}",
        outcome.rows
    );
    Ok(())
}

#[test]
fn pin_mirror_rejects_symlinked_head_inputs() -> TestResult {
    let cases = [
        (INVENTORY, "inventory-shape", INVENTORY),
        (POLICY, "policy-header", "version-policy.toml"),
        (CATALOG, "local-pin", CATALOG),
        (".velnor", "inventory-shape", INVENTORY),
        ("crates/velnor-actions-mise", "local-pin", CATALOG),
    ];
    for (linked, check, subject) in cases {
        let fixture = Fixture::new()?;
        let held = fixture.path("held");
        fs::rename(fixture.path(linked), &held)?;
        symlink(&held, fixture.path(linked))?;
        let outcome = fixture.run(None)?;
        assert!(!outcome.success, "symlinked {linked} accepted");
        let row = outcome
            .row(check, "fail", subject)
            .ok_or_else(|| format!("symlinked {linked} not named: {:?}", outcome.rows))?;
        assert!(
            row["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains("symlink")),
            "symlinked {linked} rejected for another reason: {row}"
        );
    }
    Ok(())
}

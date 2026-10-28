// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "operator_toolchain_guard_fixture.rs"]
mod fixture;

use std::{error::Error, fs};

#[test]
fn unguarded_cli_fixture_qualifies_before_guard_tests() -> Result<(), Box<dyn Error>> {
    let fixture = fixture::Fixture::new()?;
    let plan = fixture.invoke(&["plan"])?;
    assert!(plan.status.success(), "{}", fixture::stderr(&plan));
    let output = fixture.root.join("legacy-generated");
    let generated = fixture.invoke(&[
        "generate",
        "--output-dir",
        output.to_str().ok_or("non-UTF-8 output path")?,
    ])?;
    assert!(
        generated.status.success(),
        "{}",
        fixture::stderr(&generated)
    );
    assert!(
        output
            .join(".github/workflows/binary-preview.yml")
            .is_file()
    );
    let policy = fixture.root.join("legacy-policy.json");
    let exported = fixture.source_export_unguarded(&policy)?;
    assert!(exported.status.success(), "{}", fixture::stderr(&exported));
    let checked_policy = fixture.repo.join(".velnor/homebrew-release-policy.json");
    fs::write(&checked_policy, fs::read(&policy)?)?;
    let checked = fixture.invoke(&[
        "homebrew",
        "source-policy",
        "--root",
        fixture.repo.to_str().ok_or("non-UTF-8 repo path")?,
        "--check",
    ])?;
    assert!(checked.status.success(), "{}", fixture::stderr(&checked));
    Ok(())
}

#[test]
fn guarded_generate_and_plan_succeed_and_denial_writes_nothing() -> Result<(), Box<dyn Error>> {
    let fixture = fixture::Fixture::new()?;
    let plan = fixture.invoke(&[
        "--toolchain-guard",
        fixture.guard.to_str().ok_or("non-UTF-8 guard path")?,
        "plan",
    ])?;
    assert!(plan.status.success(), "{}", fixture::stderr(&plan));
    assert!(String::from_utf8_lossy(&plan.stdout).contains("Workflow to generate"));

    let output = fixture.root.join("generated");
    let generated = fixture.generate("allow", &output)?;
    assert!(
        generated.status.success(),
        "{}",
        fixture::stderr(&generated)
    );
    let workflow = fs::read_to_string(output.join(".github/workflows/binary-preview.yml"))?;
    assert!(
        workflow.contains("probe"),
        "preview workflow omitted the probe product"
    );

    let denied_output = fixture.root.join("denied-generation");
    let denied = fixture.generate("deny", &denied_output)?;
    assert!(!denied.status.success());
    assert!(fixture::stderr(&denied).contains("operator toolchain guard rejected metadata"));
    assert!(!denied_output.exists());

    let denied_plan = fixture.plan("deny")?;
    assert!(!denied_plan.status.success());
    assert!(fixture::stderr(&denied_plan).contains("operator toolchain guard rejected metadata"));
    assert_eq!(denied_plan.stdout, b"");
    Ok(())
}

#[test]
fn source_policy_export_and_check_use_guard_and_preserve_policy() -> Result<(), Box<dyn Error>> {
    let fixture = fixture::Fixture::new()?;
    let exported = fixture.root.join("source-policy.json");
    let result = fixture.invoke(&[
        "--toolchain-guard",
        fixture.guard.to_str().ok_or("non-UTF-8 guard path")?,
        "homebrew",
        "source-policy",
        "--root",
        fixture.repo.to_str().ok_or("non-UTF-8 repo path")?,
        "--output",
        exported.to_str().ok_or("non-UTF-8 output path")?,
    ])?;
    assert!(result.status.success(), "{}", fixture::stderr(&result));
    let expected = fs::read(&exported)?;
    let policy_value: serde_json::Value = serde_json::from_slice(&expected)?;
    assert_eq!(policy_value["repository"], "example/probe");
    assert_eq!(policy_value["channel"], "preview");
    assert_eq!(policy_value["version"]["version_package"], "probe");
    assert_eq!(policy_value["products"][0]["package"], "probe");
    let policy = fixture.repo.join(".velnor/homebrew-release-policy.json");
    fs::write(&policy, &expected)?;

    let denied_check = fixture.source_check("deny")?;
    assert!(!denied_check.status.success());
    assert!(fixture::stderr(&denied_check).contains("operator toolchain guard rejected metadata"));
    assert_eq!(fs::read(&policy)?, expected);

    let checked = fixture.source_check("allow")?;
    assert!(checked.status.success(), "{}", fixture::stderr(&checked));
    assert_eq!(fs::read(policy)?, expected);

    let denied_output = fixture.root.join("denied-policy.json");
    let denied = fixture.source_export("deny", &denied_output)?;
    assert!(!denied.status.success());
    assert!(fixture::stderr(&denied).contains("operator toolchain guard rejected metadata"));
    assert!(!denied_output.exists());
    Ok(())
}

#[test]
fn invalid_or_unsupported_guard_is_rejected_before_output_writes() -> Result<(), Box<dyn Error>> {
    let fixture = fixture::Fixture::new()?;
    let relative_output = fixture.root.join("relative-guard");
    let relative = fixture.generate_with_guard("relative-guard", &relative_output)?;
    assert!(!relative.status.success());
    assert!(
        fixture::stderr(&relative).contains("operator toolchain guard requires an absolute path")
    );
    assert!(!relative_output.exists());

    let missing_output = fixture.root.join("missing-guard");
    let missing_guard = fixture.root.join("bin/missing");
    let missing = fixture.generate_with_guard(
        missing_guard.to_str().ok_or("non-UTF-8 guard path")?,
        &missing_output,
    )?;
    assert!(!missing.status.success());
    assert!(
        fixture::stderr(&missing)
            .contains("operator toolchain guard requires an executable regular file")
    );
    assert!(!missing_output.exists());

    let unsupported_output = fixture.root.join("unsupported-preview");
    let unsupported = fixture.invoke(&[
        "--toolchain-guard",
        fixture.guard.to_str().ok_or("non-UTF-8 guard path")?,
        "generate",
        "--owned-tool-candidates-only",
        "--output-dir",
        unsupported_output.to_str().ok_or("non-UTF-8 output path")?,
    ])?;
    assert!(!unsupported.status.success());
    assert!(
        fixture::stderr(&unsupported)
            .contains("operator toolchain guard is unsupported for this command")
    );
    assert!(!unsupported_output.exists());
    Ok(())
}

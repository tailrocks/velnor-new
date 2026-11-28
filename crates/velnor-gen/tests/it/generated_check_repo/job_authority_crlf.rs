// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::process::Command;

use serde_json::Value;
use velnor_model::JOB_AUTHORITY_INVENTORY_PATH;
use velnor_rust::sha256_hex;

use super::job_authority_identity::written_tree;
use super::{Fixture, WORKFLOW, fixture, git, text};
use crate::facts::TestResult;

fn blob(fixture: &Fixture) -> Result<Vec<u8>, Box<dyn Error>> {
    let output = Command::new(crate::setup_action::program("git"))
        .args(["cat-file", "blob", &format!(":{WORKFLOW}")])
        .current_dir(fixture.repo())
        .output()?;
    assert!(output.status.success(), "{}", text(&output.stderr));
    Ok(output.stdout)
}

fn checked_out_as_crlf(fixture: &Fixture) -> TestResult {
    fixture.write(".gitattributes", ".github/workflows/*.yml text eol=crlf\n")?;
    git(&fixture.repo(), &["add", "--all"])?;
    std::fs::remove_file(fixture.repo().join(WORKFLOW))?;
    git(&fixture.repo(), &["checkout", "--", WORKFLOW])?;
    assert!(fixture.read(WORKFLOW)?.contains("\r\n"));
    Ok(())
}

#[test]
fn an_eol_converting_checkout_hashes_the_bytes_git_stores() -> TestResult {
    let fixture = fixture()?;
    written_tree(&fixture)?;
    checked_out_as_crlf(&fixture)?;
    let stored = blob(&fixture)?;
    assert!(!stored.contains(&b'\r'));
    let output = fixture.velnor_gen("check")?;
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    let inventory: Value =
        serde_json::from_str(&fixture.read(JOB_AUTHORITY_INVENTORY_PATH.as_str())?)?;
    assert_eq!(
        inventory["workflows"][0]["content_sha256"],
        sha256_hex(&stored)
    );
    Ok(())
}

#[test]
fn crlf_bytes_that_git_does_not_store_are_refused_naming_the_file() -> TestResult {
    let fixture = fixture()?;
    written_tree(&fixture)?;
    checked_out_as_crlf(&fixture)?;
    git(&fixture.repo(), &["rm", "--cached", "--quiet", WORKFLOW])?;
    let output = fixture.velnor_gen("check")?;
    let stderr = text(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains(WORKFLOW), "{stderr}");
    assert!(stderr.contains("git stores"), "{stderr}");
    Ok(())
}

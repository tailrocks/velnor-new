// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::expected_with_notes;

use crate::facts::{TestResult, path};
use crate::private_sources::{SECTION, private_repo};

const HTTP_LOCK: &str = "version = 4\n\n[[package]]\nname = \"package-0\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"shared\"\nversion = \"1.0.0\"\nsource = \"git+http://ghe.example/acme/shared-lib?branch=main#0123456789abcdef\"\n";

fn saves_base_caches(helper: &str, lock: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let mut repo = private_repo(&["Cargo.toml"], SECTION, helper)?;
    repo.files
        .insert(path("Cargo.lock")?, lock.as_bytes().to_vec());
    crate::facts::lock_pinned_tools(&mut repo)?;
    let (mut files, _) = expected_with_notes(&repo)?;
    let text = files
        .remove(&path(".github/workflows/ci.yml")?)
        .ok_or("no workflow")?;
    Ok(text.contains("Save the target cache"))
}

#[test]
fn an_http_git_source_gets_the_private_base_caches_from_the_helper_that_admits_it() -> TestResult {
    assert!(saves_base_caches("0.1.114", HTTP_LOCK)?);
    assert!(!saves_base_caches("0.1.113", HTTP_LOCK)?);
    Ok(())
}

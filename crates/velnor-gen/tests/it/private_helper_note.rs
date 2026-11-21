// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::pipeline::workflow;
use crate::private_sources::{SECTION, private_repo};

const ROOTS: &[&str] = &["Cargo.toml", "tools/Cargo.toml"];
const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn assets() -> String {
    format!(
        "[[release_assets]]\nrepo = 'acme/fixtures'\ntag = 'fixtures-2026.11.1'\nasset = 'dataset.tar.zst'\nsha256 = '{SHA}'\ndest = 'tests/fixtures/dataset'\n"
    )
}

fn refusal(section: &str, helper: &str) -> Result<String, Box<dyn std::error::Error>> {
    let repo = private_repo(ROOTS, section, helper)?;
    Ok(match workflow(&repo) {
        Ok(_) => String::new(),
        Err(error) => format!("{error:?}"),
    })
}

#[test]
fn an_older_helper_with_private_sources_is_refused_before_it_mints_the_token()
-> Result<(), Box<dyn std::error::Error>> {
    let refused = refusal(SECTION, "0.1.110")?;
    assert!(refused.contains("PrivateTokenHelperTooOld"), "{refused}");
    Ok(())
}

#[test]
fn an_older_helper_with_release_assets_is_refused_before_it_mints_the_token()
-> Result<(), Box<dyn std::error::Error>> {
    let refused = refusal(&format!("{SECTION}{}", assets()), "0.1.110")?;
    assert!(refused.contains("PrivateTokenHelperTooOld"), "{refused}");
    Ok(())
}

#[test]
fn private_sources_render_on_the_first_helper_that_mints_the_token_first()
-> Result<(), Box<dyn std::error::Error>> {
    let refused = refusal(SECTION, "0.1.111")?;
    assert!(!refused.contains("HelperTooOld"), "{refused}");
    Ok(())
}

#[test]
fn release_assets_render_on_the_first_helper_that_mints_the_token_first()
-> Result<(), Box<dyn std::error::Error>> {
    let refused = refusal(&format!("{SECTION}{}", assets()), "0.1.111")?;
    assert!(!refused.contains("HelperTooOld"), "{refused}");
    Ok(())
}

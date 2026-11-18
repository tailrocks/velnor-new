// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::ReleaseError;

use crate::fixture::{TestResult, files, plz_edited, refusal};

const PROXY: &str = "[http]\nproxy = \"http://proxy.example:8080\"\n";

#[test]
fn a_proxy_in_the_cargo_config_of_an_unpublished_root_is_accepted() -> TestResult {
    let mut files = plz_edited("publish_timeout = \"5m\"\n", "publish = false\n")?;
    files
        .cargo_configs
        .insert(".cargo/config.toml".to_owned(), PROXY.to_owned());
    assert_eq!(refusal(&files), None);
    Ok(())
}

#[test]
fn a_proxy_in_a_cargo_config_that_reaches_a_publishing_root_is_refused() {
    let mut files = files();
    files
        .cargo_configs
        .insert(".cargo/config.toml".to_owned(), PROXY.to_owned());
    assert!(matches!(
        refusal(&files),
        Some(ReleaseError::CargoRedirectsCratesIo { .. })
    ));
}

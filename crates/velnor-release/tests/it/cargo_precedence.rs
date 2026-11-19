// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::ReleaseError;

use crate::fixture::{files, refusal};

const OFFLINE: &str = "[net]\noffline = true\n";
const ONLINE: &str = "[net]\nretry = 3\n";

fn with_configs(extensionless: &str, toml: &str) -> velnor_release::ReleaseFiles {
    let mut files = files();
    files
        .cargo_configs
        .insert(".cargo/config".to_owned(), extensionless.to_owned());
    files
        .cargo_configs
        .insert(".cargo/config.toml".to_owned(), toml.to_owned());
    files
}

#[test]
fn offline_in_the_config_toml_cargo_ignores_is_not_refused() {
    assert_eq!(refusal(&with_configs(ONLINE, OFFLINE)), None);
}

#[test]
fn offline_in_the_extensionless_config_cargo_reads_is_refused() {
    assert!(matches!(
        refusal(&with_configs(OFFLINE, ONLINE)),
        Some(ReleaseError::CargoOffline { .. })
    ));
}

#[test]
fn offline_in_a_lone_config_toml_is_still_refused() {
    let mut lone = files();
    lone.cargo_configs
        .insert(".cargo/config.toml".to_owned(), OFFLINE.to_owned());
    assert!(matches!(
        refusal(&lone),
        Some(ReleaseError::CargoOffline { .. })
    ));
}

#[test]
fn a_proxy_in_the_ignored_config_toml_is_still_refused() {
    let config = with_configs(ONLINE, "[http]\nproxy = \"http://p.example:1\"\n");
    assert!(matches!(
        refusal(&config),
        Some(ReleaseError::CargoRedirectsCratesIo { .. })
    ));
}

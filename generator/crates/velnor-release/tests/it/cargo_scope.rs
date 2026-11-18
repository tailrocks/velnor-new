// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::ReleaseError;

use crate::fixture::refusal;
use crate::sources::two_roots;

const PROXY: &str = "[http]\nproxy = \"http://proxy.example:8080\"\n";

fn publishing_root_and_unpublished_tools() -> Result<velnor_release::ReleaseFiles, String> {
    let tools = "[workspace]\ngit_tag_name = \"{{ package }}-v{{ version }}\"\ngit_release_draft = true\nrelease_always = false\nsemver_check = false\npublish = false\npr_branch_prefix = \"tools-\"\n\n[[package]]\nname = \"tools\"\n";
    two_roots(tools)
}

#[test]
fn a_proxy_in_the_cargo_config_of_an_unpublished_root_is_accepted() -> Result<(), String> {
    let mut files = publishing_root_and_unpublished_tools()?;
    files
        .cargo_configs
        .insert("tools/.cargo/config.toml".to_owned(), PROXY.to_owned());
    assert_eq!(refusal(&files), None);
    Ok(())
}

#[test]
fn a_proxy_in_a_cargo_config_that_reaches_a_publishing_root_is_refused() -> Result<(), String> {
    let mut files = publishing_root_and_unpublished_tools()?;
    files
        .cargo_configs
        .insert(".cargo/config.toml".to_owned(), PROXY.to_owned());
    assert!(matches!(
        refusal(&files),
        Some(ReleaseError::CargoRedirectsCratesIo { .. })
    ));
    Ok(())
}

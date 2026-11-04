// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{ffi::OsString, fs, os::unix::fs::PermissionsExt as _, path::Path, time::Duration};
use velnor_actions_mise::{
    command::{CratePublishCredential, IsolatedCommand},
    release_git_pin::verify_bundle,
};

#[test]
fn real_pinned_git_is_used_by_update_and_publish_children() -> Result<(), Box<dyn std::error::Error>>
{
    let bundle =
        std::env::var_os("VELNOR_TEST_GIT_BUNDLE").ok_or("VELNOR_TEST_GIT_BUNDLE is required")?;
    let git = verify_bundle(Path::new(&bundle))?;
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let source = root.join("source");
    fs::create_dir(&source)?;
    let config = source.join("release-plz.toml");
    let manifest = source.join("Cargo.toml");
    fs::write(&config, "")?;
    fs::write(&manifest, "")?;
    let program = root.join("release-plz");
    fs::write(
        &program,
        r#"#!/bin/sh
set -eu
test "$(command -v git)" = "$GIT_EXEC_PATH/git"
test "$(git --exec-path)" = "$GIT_EXEC_PATH"
git --version >/dev/null
case "$1" in
  update) test -z "${GIT_TOKEN+x}${CARGO_REGISTRY_TOKEN+x}${ACTIONS_ID_TOKEN_REQUEST_TOKEN+x}" ;;
  release) test -n "$GIT_TOKEN"; test -n "$CARGO_REGISTRY_TOKEN"; test -z "${ACTIONS_ID_TOKEN_REQUEST_TOKEN+x}" ;;
  *) exit 1 ;;
esac
"#,
    )?;
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755))?;
    let home = root.to_str().ok_or("temporary path")?;
    let environment: Vec<(OsString, OsString)> = [
        ("PATH", "/usr/bin:/bin"),
        ("CARGO", "/usr/bin/true"),
        ("RUSTC", "/usr/bin/true"),
        ("RUSTDOC", "/usr/bin/true"),
        ("HOME", home),
        ("CARGO_HOME", home),
        ("CARGO_TARGET_DIR", home),
        ("RUSTUP_HOME", home),
        ("RUSTUP_AUTO_INSTALL", "0"),
    ]
    .into_iter()
    .map(|(key, value)| (key.into(), value.into()))
    .collect();
    let args = vec![
        "update".into(),
        "--config".into(),
        config.into_os_string(),
        "--manifest-path".into(),
        manifest.into_os_string(),
    ];
    let update = IsolatedCommand::crate_update(&program, args, &source, &environment, &git)?;
    assert!(update.run_bounded(4096, Duration::from_secs(10))?.success);
    let publish_env: Vec<_> = environment
        .into_iter()
        .filter(|(key, _)| key != "CARGO_TARGET_DIR")
        .collect();
    let publish = IsolatedCommand::crate_publish(
        &program,
        vec!["release".into()],
        &source,
        &publish_env,
        "fixture_git",
        CratePublishCredential::Bootstrap {
            cargo_token: "fixture_registry",
        },
        &git,
    )?;
    assert!(publish.run_bounded(4096, Duration::from_secs(10))?.success);
    Ok(())
}

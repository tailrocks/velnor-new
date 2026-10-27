// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[test]
fn maximum_member_id_round_trips_verified_assets() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let runner = tempfile::tempdir()?;
    fs::create_dir(workspace.path().join(".velnor"))?;
    fs::create_dir(runner.path().join("velnor"))?;
    let id = format!("bun-{}-{}", "a".repeat(64), "b".repeat(64));
    assert_eq!(id.len(), 133);
    success(workspace.path(), runner.path(), &id, "prepare")?;
    let deps = workspace.path().join(".velnor/deps");
    fs::create_dir_all(deps.join("assets"))?;
    let payload = b"real handoff payload\0with binary content\xff";
    fs::write(deps.join("assets/data.bin"), payload)?;
    let marker = serde_json::to_vec(&serde_json::json!({
        "schema": 1,
        "sources": [{
            "kind": "release-assets", "id": "fixture-assets", "path": "assets",
            "repository": "fixture-owner/assets", "tag": "v1.0.0",
            "commit": "0123456789abcdef0123456789abcdef01234567",
            "asset": "assets.tar.gz", "sha256": DIGEST
        }]
    }))?;
    fs::write(deps.join("verified.json"), &marker)?;
    success(workspace.path(), runner.path(), &id, "export")?;
    let stage = runner.path().join("velnor/bun-assets").join(&id);
    let archive = stage.join(format!("velnor-pw-assets-{id}.tar.gz"));
    let bytes = fs::read(&archive)?;
    success(workspace.path(), runner.path(), &id, "prepare")?;
    assert!(!deps.exists());
    assert!(!archive.exists());
    fs::write(archive, bytes)?;
    success(workspace.path(), runner.path(), &id, "import")?;
    assert_eq!(fs::read(deps.join("assets/data.bin"))?, payload);
    assert_eq!(fs::read(deps.join("verified.json"))?, marker);
    assert!(!stage.exists(), "import must remove its staging directory");
    Ok(())
}

#[test]
fn invalid_member_ids_fail_before_staging_or_workspace_mutation() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let runner = tempfile::tempdir()?;
    fs::create_dir(workspace.path().join(".velnor"))?;
    fs::create_dir(runner.path().join("velnor"))?;
    for id in [
        "x".repeat(134),
        String::new(),
        "../escape".to_owned(),
        "a/b".to_owned(),
    ] {
        let output = run(workspace.path(), runner.path(), &id, "prepare")?;
        assert!(!output.status.success(), "invalid ID was accepted: {id:?}");
        let stderr = String::from_utf8(output.stderr)?;
        assert!(stderr.contains("bun_assets_id_invalid"), "{stderr}");
        assert!(!runner.path().join("velnor/bun-assets").exists());
        assert!(!workspace.path().join(".velnor/deps").exists());
    }
    Ok(())
}

fn success(
    workspace: &Path,
    runner: &Path,
    id: &str,
    operation: &str,
) -> Result<(), Box<dyn Error>> {
    let output = run(workspace, runner, id, operation)?;
    assert!(
        output.status.success(),
        "{operation}: {}",
        String::from_utf8(output.stderr)?
    );
    Ok(())
}

fn run(
    workspace: &Path,
    runner: &Path,
    id: &str,
    operation: &str,
) -> Result<Output, Box<dyn Error>> {
    Ok(Command::new(env!("CARGO_BIN_EXE_velnor-actions"))
        .env_clear()
        .env("GITHUB_WORKSPACE", workspace)
        .env("RUNNER_TEMP", runner)
        .env("VELNOR_INTERNAL_OP", format!("bun-assets-{operation}-v1"))
        .env("VELNOR_BUN_ASSET_ID", id)
        .env("VELNOR_BUN_ASSET_PINS", format!("fixture-assets:{DIGEST}"))
        .output()?)
}

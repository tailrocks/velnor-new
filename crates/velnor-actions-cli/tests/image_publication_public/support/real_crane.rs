// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "real_crane/manifest_lookup.rs"]
mod manifest_lookup;
#[path = "real_crane/raw_blob.rs"]
mod raw_blob;
#[path = "real_crane/registry.rs"]
mod registry;

use std::{
    collections::BTreeMap,
    error::Error,
    ffi::{OsStr, OsString},
    path::Path,
    time::Duration,
};

use velnor_actions_mise::{
    CheckDeadline, IsolatedCommand, PinnedTool, PinnedToolExec, ToolCatalog,
};

use super::oci;
use registry::Registry;

pub(super) fn crane_command(args: &[&str]) -> Result<IsolatedCommand, Box<dyn Error>> {
    let request = PinnedToolExec::new(
        vec![PinnedTool::Crane],
        OsStr::new("crane"),
        args.iter().map(OsString::from).collect(),
    )?;
    let local_proxy_bypass = [
        (
            OsString::from("NO_PROXY"),
            OsString::from("127.0.0.1,localhost"),
        ),
        (
            OsString::from("no_proxy"),
            OsString::from("127.0.0.1,localhost"),
        ),
    ];
    Ok(request
        .command(&ToolCatalog::pinned())?
        .with_env(&local_proxy_bypass)?)
}

pub(super) fn crane(args: &[&str]) -> Result<velnor_actions_mise::ProcessOutput, Box<dyn Error>> {
    let command = crane_command(args)?;
    let (output, _) =
        command.run_quiescent_until(1024 * 1024, CheckDeadline::after(Duration::from_secs(90))?)?;
    Ok(output)
}

pub(super) fn pinned_crane_version() -> Result<String, Box<dyn Error>> {
    let version = crane(&["version"])?;
    require_success(&version, "crane version");
    let actual = String::from_utf8(version.stdout)?;
    let catalog = ToolCatalog::pinned();
    let expected = catalog.version(PinnedTool::Crane).to_owned();
    assert!(
        actual.contains(&expected),
        "unexpected pinned Crane version: {actual}"
    );
    Ok(expected)
}

pub(super) fn require_success(output: &velnor_actions_mise::ProcessOutput, operation: &str) {
    assert!(
        output.success,
        "{operation} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

pub(super) fn path_text(path: &Path) -> Result<&str, Box<dyn Error>> {
    path.to_str()
        .ok_or_else(|| "test layout path is not UTF-8".into())
}

fn assert_published_bytes(
    registry: &Registry,
    layout: &oci::Layout,
    digest: &str,
) -> Result<(), Box<dyn Error>> {
    let manifest_path = format!(
        "blobs/sha256/{}",
        digest
            .strip_prefix("sha256:")
            .ok_or("manifest digest is not prefixed")?
    );
    let expected_manifest = layout
        .inventory
        .get(&manifest_path)
        .ok_or("fixture manifest bytes are missing")?;
    assert_eq!(
        registry.manifest_bytes(digest)?,
        Some(expected_manifest.clone())
    );
    assert_eq!(
        registry.stored_inventory()?,
        published_blob_inventory(layout)
    );
    Ok(())
}

pub(super) fn published_blob_inventory(layout: &oci::Layout) -> BTreeMap<String, Vec<u8>> {
    let manifest = format!(
        "blobs/sha256/{}",
        layout
            .identity
            .platform_manifest_digest
            .trim_start_matches("sha256:")
    );
    layout
        .inventory
        .iter()
        .filter(|(path, _)| path.as_str() != manifest.as_str())
        .map(|(path, bytes)| (path.clone(), bytes.clone()))
        .collect()
}

use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::Output;

pub(super) const REPOSITORY: &str = "tailrocks/velnor-new";
pub(super) const RELEASE_VERSION: &str = "0.1.0";
pub(super) const MANIFEST_NAME: &str = "velnor-actions-release-manifest.json";
pub(super) const MANIFEST_CHECKSUM_NAME: &str = "velnor-actions-release-manifest.json.sha256";
pub(super) const LINUX_TARGET: &str = "x86_64-unknown-linux-gnu";
pub(super) const MACOS_ARM64_TARGET: &str = "aarch64-apple-darwin";
pub(super) const MACOS_X86_64_TARGET: &str = "x86_64-apple-darwin";
pub(super) const LINUX_SHA: &str =
    "2e43e05be6002cfe3837d95b0501673bc2ab21f9d1ad7f97da87b427dcdd82b0";
pub(super) const MACOS_ARM64_SHA: &str =
    "f690c2a737e988a7d26b6e718daf2fc37f86ab07400a5e70d6bfa3e626658f58";
pub(super) const MACOS_X86_64_SHA: &str =
    "c5159fd4a7361f789c013bb3e076a7afbb10af4e669a2917eddb980355efed53";
pub(super) const LINUX_SIDECAR_SHA: &str =
    "d57170b4b32945ec812c9206e0fb24fbe89d2704fa44e362877d7fc04a70b248";
pub(super) const MACOS_ARM64_SIDECAR_SHA: &str =
    "73d6fb595617adc0d5ee5f3a399899e2f0d8dcbc312730710f4731fe76c3bba8";
pub(super) const MACOS_X86_64_SIDECAR_SHA: &str =
    "da5be66af89449ed2ef74a739c884b71454a8cc98b9aab5a82958a1dbad957de";

/// Canonical target fixtures with the staging directory used by the publisher.
pub(super) const TARGET_FIXTURES: &[(&str, &str, &[u8], &str, &str)] = &[
    (
        LINUX_TARGET,
        "linux-assets",
        b"linux binary fixture\n",
        LINUX_SHA,
        LINUX_SIDECAR_SHA,
    ),
    (
        MACOS_ARM64_TARGET,
        "macos-assets",
        b"macos binary fixture\n",
        MACOS_ARM64_SHA,
        MACOS_ARM64_SIDECAR_SHA,
    ),
    (
        MACOS_X86_64_TARGET,
        "macos-x86_64-assets",
        b"macos intel binary fixture\n",
        MACOS_X86_64_SHA,
        MACOS_X86_64_SIDECAR_SHA,
    ),
];

pub(super) fn write_local_assets(directory: &Path, version: &str) -> Result<(), Box<dyn Error>> {
    for (target, _source_directory, bytes, digest, _sidecar_digest) in
        TARGET_FIXTURES.iter().copied()
    {
        let name = format!("velnor-actions-{version}-{target}");
        fs::write(directory.join(&name), bytes)?;
        fs::write(
            directory.join(format!("{name}.sha256")),
            format!("{digest}  {name}\n"),
        )?;
    }
    Ok(())
}

pub(super) fn asset_records(version: &str, tag: &str) -> Vec<String> {
    let mut records = Vec::new();
    for (target, _source_directory, bytes, binary_digest, sidecar_digest) in
        TARGET_FIXTURES.iter().copied()
    {
        let name = format!("velnor-actions-{version}-{target}");
        records.push(asset_record(
            &name,
            bytes.len() as u64,
            binary_digest,
            Some(&asset_url(tag, &name)),
        ));
        let sidecar = format!("{name}.sha256");
        let sidecar_content = format!("{binary_digest}  {name}\n");
        records.push(asset_record(
            &sidecar,
            sidecar_content.len() as u64,
            sidecar_digest,
            Some(&asset_url(tag, &sidecar)),
        ));
    }
    records
}

pub(super) fn asset_records_without_url(version: &str, tag: &str) -> Vec<String> {
    let mut records = asset_records(version, tag);
    let url_start = records[0]
        .find(",\"browser_download_url\"")
        .expect("fixture URL");
    let end = records[0].rfind('}').expect("fixture object");
    records[0].replace_range(url_start..end, "");
    records
}

pub(super) fn asset_record(name: &str, size: u64, digest: &str, url: Option<&str>) -> String {
    let id = 100_000_000 + name.bytes().map(u64::from).sum::<u64>();
    let content_type = if name.ends_with(".sha256") {
        "text/plain"
    } else {
        "application/octet-stream"
    };
    let url = url.map_or_else(String::new, |value| {
        format!(",\"browser_download_url\":\"{value}\"")
    });
    format!(
        "{{\"url\":\"https://api.github.com/repos/{REPOSITORY}/releases/assets/{id}\",\"id\":{id},\"node_id\":\"RA_kwDOfixture{id}\",\"name\":\"{name}\",\"label\":null,\"uploader\":{{\"login\":\"github-actions[bot]\",\"id\":41898282,\"node_id\":\"MDM6Qm90NDE4OTgyODI=\",\"type\":\"Bot\"}},\"content_type\":\"{content_type}\",\"state\":\"uploaded\",\"size\":{size},\"digest\":\"sha256:{digest}\",\"download_count\":0,\"created_at\":\"2026-10-05T06:00:00Z\",\"updated_at\":\"2026-10-05T06:00:00Z\"{url}}}"
    )
}

pub(super) fn release_json(draft: bool, immutable: bool, tag: &str, assets: &[String]) -> String {
    let published_at = if draft {
        "null"
    } else {
        "\"2026-10-05T06:01:00Z\""
    };
    format!(
        "{{\"url\":\"https://api.github.com/repos/{REPOSITORY}/releases/741852963\",\"assets_url\":\"https://api.github.com/repos/{REPOSITORY}/releases/741852963/assets\",\"upload_url\":\"https://uploads.github.com/repos/{REPOSITORY}/releases/741852963/assets{{?name,label}}\",\"html_url\":\"https://github.com/{REPOSITORY}/releases/tag/{tag}\",\"id\":741852963,\"node_id\":\"RE_kwDOfixture\",\"tag_name\":\"{tag}\",\"target_commitish\":\"main\",\"name\":\"{tag}\",\"draft\":{draft},\"immutable\":{immutable},\"created_at\":\"2026-10-05T06:00:00Z\",\"updated_at\":\"2026-10-05T06:01:00Z\",\"published_at\":{published_at},\"prerelease\":false,\"author\":{{\"login\":\"github-actions[bot]\",\"id\":41898282,\"node_id\":\"MDM6Qm90NDE4OTgyODI=\",\"type\":\"Bot\"}},\"assets\":[{}]}}",
        assets.join(",")
    )
}

pub(super) fn asset_url(tag: &str, asset: &str) -> String {
    format!("https://github.com/{REPOSITORY}/releases/download/{tag}/{asset}")
}

pub(super) fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

pub(super) fn assert_failure(output: &Output, reason: &str) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "expected {reason}: {stderr}");
    assert!(stderr.contains(reason), "expected {reason}: {stderr}");
}

pub(super) fn output_text(output: &Output) -> Result<String, Box<dyn Error>> {
    Ok(String::from_utf8(output.stdout.clone())?)
}

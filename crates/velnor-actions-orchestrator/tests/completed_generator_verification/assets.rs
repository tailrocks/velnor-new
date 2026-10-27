// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs, path::Path, process::Command};

use serde_json::json;

pub(super) const SOURCE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub(super) const TARGETS: [(&str, &str); 3] = [
    ("x86_64-unknown-linux-gnu", "linux-assets"),
    ("aarch64-apple-darwin", "macos-assets"),
    ("x86_64-apple-darwin", "macos-intel-assets"),
];
const MANIFEST_SCRIPT: &str =
    include_str!("../../../../scripts/generator-release/create-release-manifest.sh");

pub(super) fn binary_name(target: &str) -> String {
    format!("velnor-actions-{}-{target}", env!("CARGO_PKG_VERSION"))
}

pub(super) fn create(root: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(root.join("release-attestations"))?;
    for (target, directory) in TARGETS {
        fs::create_dir_all(root.join(directory))?;
        let name = binary_name(target);
        let bytes = format!("UNVERIFIED passive integrity fixture for {target}\n");
        let digest = crate::sha256(bytes.as_bytes());
        fs::write(root.join(directory).join(&name), bytes)?;
        fs::write(
            root.join(directory).join(format!("{name}.sha256")),
            format!("{digest}  {name}\n"),
        )?;
        let provenance = json!({
            "schema": 1, "version": env!("CARGO_PKG_VERSION"),
            "repository": "tailrocks/velnor-new", "commit": SOURCE,
            "target": target, "asset": name, "sha256": digest,
            "toolchain": {"rust": "1.91.0", "mr-boxington": "1.0.0"}
        });
        fs::write(
            root.join(directory).join(format!("{name}.provenance.json")),
            serde_json::to_vec(&provenance)?,
        )?;
        for subject in [
            &name,
            &format!("{name}.sha256"),
            &format!("{name}.provenance.json"),
        ] {
            write_bundle(root, subject)?;
        }
    }
    let script = root.join("create-manifest.sh");
    fs::write(&script, MANIFEST_SCRIPT)?;
    let output = Command::new("bash")
        .arg(&script)
        .args([
            env!("CARGO_PKG_VERSION"),
            "tailrocks/velnor-new",
            "1.91.0",
            "1.0.0",
        ])
        .current_dir(root)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").ok_or("PATH missing")?)
        .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
        .env("GITHUB_SHA", SOURCE)
        .output()?;
    assert!(
        output.status.success(),
        "owned manifest fixture preparation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fs::remove_file(script)?;
    fs::create_dir_all(root.join("manifest-assets"))?;
    fs::rename(
        root.join("release-manifest.json"),
        root.join("manifest-assets/release-manifest.json"),
    )?;
    write_bundle(root, "release-manifest.json")?;
    Ok(())
}

fn write_bundle(root: &Path, subject: &str) -> Result<(), Box<dyn Error>> {
    fs::write(
        root.join("release-attestations")
            .join(format!("{subject}.intoto.jsonl")),
        "UNVERIFIED passive bundle; external attestation boundary must reject\n",
    )?;
    Ok(())
}

pub(super) fn release_response(root: &Path) -> Result<serde_json::Value, Box<dyn Error>> {
    let mut assets = Vec::new();
    for directory in [
        "linux-assets",
        "macos-assets",
        "macos-intel-assets",
        "manifest-assets",
        "release-attestations",
    ] {
        for entry in fs::read_dir(root.join(directory))? {
            let path = entry?.path();
            let bytes = fs::read(&path)?;
            assets.push(json!({"name": path.file_name().ok_or("missing asset name")?.to_str().ok_or("nonutf8 asset")?, "state": "uploaded", "size": bytes.len(), "digest": format!("sha256:{}", crate::sha256(&bytes))}));
        }
    }
    Ok(
        json!([[{"tag_name": format!("v{}", env!("CARGO_PKG_VERSION")), "target_commitish": SOURCE, "draft": false, "prerelease": false, "immutable": true, "assets": assets}]]),
    )
}

pub(super) fn corrupt(root: &Path, kind: &str) -> Result<(), Box<dyn Error>> {
    let binary = binary_name(TARGETS[0].0);
    match kind {
        "binary" => fs::write(
            root.join("linux-assets").join(binary),
            "corrupted passive bytes",
        )?,
        "source" | "toolchain" => {
            let path = root
                .join("linux-assets")
                .join(format!("{binary}.provenance.json"));
            let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
            if kind == "source" {
                value["commit"] = json!("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
            } else {
                value["toolchain"]["rust"] = json!("0.0.0");
            }
            fs::write(path, serde_json::to_vec(&value)?)?;
        }
        "manifest" => fs::write(root.join("manifest-assets/release-manifest.json"), "{}\n")?,
        _ => return Err("unknown corruption".into()),
    }
    Ok(())
}

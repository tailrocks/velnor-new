// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::reviewed_generator_manifest;

#[test]
fn generator_pin_uses_validated_exact_manifest_bytes() -> Result<(), Box<dyn std::error::Error>> {
    let version = env!("CARGO_PKG_VERSION");
    let source = "b".repeat(40);
    let digest = "a".repeat(64);
    let targets: Vec<_> = ["x86_64-unknown-linux-gnu", "aarch64-apple-darwin", "x86_64-apple-darwin"]
        .into_iter().map(|target| serde_json::json!({"target": target, "artifact": format!("https://github.com/tailrocks/velnor-new/releases/download/v{version}/velnor-actions-{version}-{target}"), "sha256": digest})).collect();
    let manifest = serde_json::json!({"schema": 1, "version": version, "repository": "tailrocks/velnor-new", "commit": source, "targets": targets}).to_string();
    let pin = reviewed_generator_manifest(Some(&manifest), false, version)?;
    assert_eq!(pin.0, source);
    let changed = reviewed_generator_manifest(Some(&(manifest.clone() + "\n")), false, version)?;
    assert_eq!(changed.0, pin.0);
    assert_ne!(changed.1, pin.1);
    assert!(reviewed_generator_manifest(Some(&manifest), true, version).is_err());
    assert!(reviewed_generator_manifest(None, false, version).is_err());
    assert!(reviewed_generator_manifest(Some(&manifest), false, "0.1.99").is_err());
    let mut invalid: serde_json::Value = serde_json::from_str(&manifest)?;
    invalid["commit"] = serde_json::json!("not-a-commit");
    assert!(reviewed_generator_manifest(Some(&invalid.to_string()), false, version).is_err());
    invalid["commit"] = serde_json::json!(source);
    invalid["targets"][0]["target"] = serde_json::json!("unsupported");
    assert!(reviewed_generator_manifest(Some(&invalid.to_string()), false, version).is_err());
    Ok(())
}

#[test]
fn generator_pin_uses_exact_official_release_manifest() -> Result<(), Box<dyn std::error::Error>> {
    let manifest = include_str!("release_emit_tests/official-0.1.12.json");
    let (source, digest) = reviewed_generator_manifest(Some(manifest), false, "0.1.12")?;
    assert_eq!(source, "f1b044a606b6bc469f9718644f5c0ed88bf965b6");
    assert_eq!(
        digest,
        "e945a9122ed051cbe918ef3208f7fa6ec2203a5228be4437a680c33a67fc1cbb"
    );
    Ok(())
}

#[test]
fn configured_features_reach_release_plz_and_cargo_package()
-> Result<(), Box<dyn std::error::Error>> {
    use std::collections::BTreeSet;
    use std::process::Command;
    use velnor_actions_contract::config::{ReleaseRegistry, RustReleaseConfig};
    use velnor_actions_rust::release_select::{ReleaseRequest, ReleaseScope, select_release_set};
    let temp = tempfile::tempdir()?;
    std::fs::create_dir(temp.path().join("src"))?;
    std::fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname='probe'\nversion='0.1.0'\nedition='2021'\n[features]\npty=[]\n",
    )?;
    std::fs::write(
        temp.path().join("src/lib.rs"),
        "#[cfg(not(feature = \"pty\"))]\ncompile_error!(\"pty required\");\n",
    )?;
    let metadata = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--offline",
        ])
        .current_dir(temp.path())
        .output()?;
    assert!(metadata.status.success());
    let metadata = String::from_utf8(metadata.stdout)?;
    let root = temp.path().canonicalize()?;
    let scope = ReleaseScope::Packages(vec!["probe".into()]);
    let empty = BTreeSet::new();
    let selected = select_release_set(&ReleaseRequest {
        metadata_json: &metadata,
        repo_root: &root,
        manifest_hint: "Cargo.toml",
        scope: &scope,
        enabled: true,
        affected: &empty,
        known_manifests: &empty,
        supported_registries: &[],
    })?;
    let release = RustReleaseConfig {
        enabled: true,
        registry: ReleaseRegistry::CratesIo,
        packages: vec!["probe".into()],
        publish_features: [("probe".into(), vec!["pty".into()])].into(),
        ..RustReleaseConfig::default()
    };
    let config = super::config::plz_config(&release, &selected)?;
    assert_eq!(config.packages[0].publish_features, ["pty"]);
    let rendered = velnor_actions_workflow_renderer::release_config::render_release_plz_config(
        &config,
        env!("CARGO_PKG_VERSION"),
    )?;
    assert!(rendered.contains("publish_features = [\"pty\"]"));
    let package = Command::new(env!("CARGO"))
        .args([
            "package",
            "--offline",
            "--features",
            &config.packages[0].publish_features.join(","),
        ])
        .current_dir(temp.path())
        .output()?;
    assert!(
        package.status.success(),
        "{}",
        String::from_utf8_lossy(&package.stderr)
    );
    let missing = Command::new(env!("CARGO"))
        .args(["package", "--offline"])
        .current_dir(temp.path())
        .output()?;
    assert!(!missing.status.success());
    let mut unknown = release;
    unknown
        .publish_features
        .insert("other".into(), vec!["pty".into()]);
    assert!(super::config::plz_config(&unknown, &selected).is_err());
    Ok(())
}

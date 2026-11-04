// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::reviewed_generator_manifest;

fn preview_consumer_intent(root: &std::path::Path) -> Result<String, crate::OrchestratorError> {
    let prep = crate::prepare::prepare_for_candidate(root)?;
    let release = super::enabled_release(&prep)?.ok_or_else(|| crate::OrchestratorError::unsupported(
        "crate_release_preview", "release is not enabled",
    ))?;
    if release.registry != velnor_actions_contract::config::ReleaseRegistry::CratesIo {
        return Err(crate::OrchestratorError::unsupported("crate_release_preview", "registry is not crates.io"));
    }
    let mise = crate::pins::resolve_mise_setup(&prep.config, super::RELEASE_HOSTED_RUNNER)?;
    let files = super::release_files_for_config(&prep, &mise, release)?;
    let mut matching = files.into_iter().filter(|file| file.path == velnor_actions_workflow_renderer::release_tree::RELEASE_INTENT_PATH);
    let intent = matching.next().ok_or_else(|| crate::OrchestratorError::unsupported("crate_release_preview", "intent missing"))?;
    if matching.next().is_some() {
        return Err(crate::OrchestratorError::unsupported("crate_release_preview", "duplicate intent"));
    }
    crate::crate_release_runtime::parse_crate_release_intent(intent.bytes.as_bytes())?;
    Ok(intent.bytes)
}

fn write_consumer_intent(root: &std::path::Path, output: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write as _;
    let root = root.canonicalize()?;
    let parent = output.parent().ok_or("output parent")?.canonicalize()?;
    if parent.starts_with(&root) || !parent.starts_with(std::env::temp_dir().canonicalize()?) {
        return Err("output must be outside the checkout and inside the temporary directory".into());
    }
    let intent = preview_consumer_intent(&root)?;
    std::fs::OpenOptions::new().write(true).create_new(true).open(output)?.write_all(intent.as_bytes())?;
    Ok(())
}

fn preview_fixture(root: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use std::{fs, process::Command};
    fs::create_dir_all(root.join(".velnor"))?;
    fs::create_dir_all(root.join("src"))?;
    fs::write(root.join(".velnor/config.toml"), "schema = 1\n[workflow]\npolicy = 'consumer-v1'\ndefault_branch = 'main'\n[stacks.rust.release]\nenabled = true\nregistry = 'crates-io'\ntrusted_reviewer_id = 123\nrequired_workflow_path = '.github/workflows/ci.yml'\npackages = ['demo']\n")?;
    fs::write(root.join("Cargo.toml"), "[package]\nname = 'demo'\nversion = '0.1.0'\nedition = '2021'\n")?;
    fs::write(root.join("Cargo.lock"), "version = 4\n\n[[package]]\nname = 'demo'\nversion = '0.1.0'\n")?;
    fs::write(root.join("src/lib.rs"), "pub fn demo() {}\n")?;
    let version = env!("CARGO_PKG_VERSION");
    let targets: Vec<_> = ["x86_64-unknown-linux-gnu", "aarch64-apple-darwin", "x86_64-apple-darwin"].into_iter().map(|target| serde_json::json!({"target": target, "artifact": format!("https://github.com/tailrocks/velnor-new/releases/download/v{version}/velnor-actions-{version}-{target}"), "sha256": "a".repeat(64)})).collect();
    fs::write(root.join(".velnor/release-manifest.json"), serde_json::json!({"schema": 1, "version": version, "repository": "tailrocks/velnor-new", "commit": "b".repeat(40), "targets": targets}).to_string())?;
    for args in [&["init", "-b", "main"][..], &["remote", "add", "origin", "https://github.com/example/demo.git"][..], &["add", "."][..], &["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-m", "fixture"][..]] {
        let result = Command::new("git").args(args).current_dir(root).output()?;
        if !result.status.success() { return Err(String::from_utf8_lossy(&result.stderr).into_owned().into()); }
    }
    Ok(())
}

#[test]
fn supplied_consumer_intent_preview() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let output = temp.path().join("intent.toml");
    assert!(write_consumer_intent(temp.path(), &output).is_err());
    assert!(!output.exists());
    let checkout = temp.path().join("checkout");
    std::fs::create_dir(&checkout)?;
    preview_fixture(&checkout)?;
    let prep = crate::prepare::prepare_for_candidate(&checkout)?;
    assert!(super::release_files(&prep).is_err());
    write_consumer_intent(&checkout, &output)?;
    let intent = crate::crate_release_runtime::parse_crate_release_intent(&std::fs::read(&output)?)?;
    assert_eq!(intent.packages, ["demo"]);
    assert!(write_consumer_intent(&checkout, &output).is_err());
    let config_path = checkout.join(".velnor/config.toml");
    let config = std::fs::read_to_string(&config_path)?;
    std::fs::write(&config_path, config.replace("packages = ['demo']", "packages = ['missing']"))?;
    let invalid_output = temp.path().join("invalid.toml");
    assert!(write_consumer_intent(&checkout, &invalid_output).is_err());
    assert!(!invalid_output.exists());
    if let Some(root) = std::env::var_os("VELNOR_TEST_PREVIEW_ROOT") {
        let output = std::env::var_os("VELNOR_TEST_PREVIEW_OUTPUT").ok_or("output path")?;
        write_consumer_intent(std::path::Path::new(&root), std::path::Path::new(&output))?;
    }
    Ok(())
}

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

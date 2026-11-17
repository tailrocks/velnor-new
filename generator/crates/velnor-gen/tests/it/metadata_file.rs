// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::{Drift, drift, generated_files};
use velnor_model::{DependencyKind, RepoPath, RepoView, TargetKind};
use velnor_rust::{
    LOCK_PATH, METADATA_PATH, MetadataFileError, metadata_file, recorded_metadata, sha256_hex,
};

use crate::facts::{TestResult, package, path};
use crate::pipeline::{HELPER_LOCK, view};

fn pin(files: &mut RepoView, version: &str) {
    let lock = String::from_utf8_lossy(HELPER_LOCK).replace("0.1.79", version);
    files.files.insert(LOCK_PATH, lock.into_bytes());
}

fn two_workspaces() -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut files = view(&["Cargo.toml", "tool/Cargo.toml"], None)?;
    pin(&mut files, "0.1.81");
    let core = package(
        "core",
        "core/Cargo.toml",
        &[],
        &[
            ("core", TargetKind::Lib, false),
            ("it", TargetKind::Test, true),
        ],
    )?;
    let app = package(
        "app",
        "Cargo.toml",
        &[("core", DependencyKind::Normal, "core/Cargo.toml")],
        &[("app", TargetKind::Bin, false)],
    )?;
    let tool = package(
        "tool",
        "tool/Cargo.toml",
        &[("core", DependencyKind::Dev, "core/Cargo.toml")],
        &[("tool", TargetKind::Bin, false)],
    )?;
    files
        .rust_metadata
        .get_mut(&path("Cargo.toml")?)
        .ok_or("root")?
        .members = vec![app, core];
    files
        .rust_metadata
        .get_mut(&path("tool/Cargo.toml")?)
        .ok_or("tool")?
        .members = vec![tool];
    for manifest in ["Cargo.toml", "core/Cargo.toml", "tool/Cargo.toml"] {
        files.tracked_paths.insert(path(manifest)?);
        files
            .files
            .insert(path(manifest)?, format!("# {manifest}\n").into_bytes());
    }
    Ok(files)
}

fn recorded(files: &RepoView) -> Result<String, Box<dyn std::error::Error>> {
    Ok(metadata_file(files)?)
}

#[test]
fn the_digest_matches_the_published_test_vectors() {
    let million = "a".repeat(1_000_000);
    for (text, digest) in [
        (
            "",
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        ),
        (
            "abc",
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        ),
        (
            "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
        ),
        (
            million.as_str(),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0",
        ),
    ] {
        assert_eq!(sha256_hex(text.as_bytes()), digest, "{}", text.len());
    }
}

#[test]
fn the_file_gives_back_every_member_edge_and_test_target_it_was_written_from() -> TestResult {
    let files = two_workspaces()?;
    let text = recorded(&files)?;
    assert_eq!(
        recorded_metadata(&files, Some(text.as_bytes()))?,
        files.rust_metadata
    );
    assert_eq!(recorded(&files)?, text);
    Ok(())
}

#[test]
fn a_manifest_that_changed_or_vanished_after_the_file_was_written_is_named() -> TestResult {
    let files = two_workspaces()?;
    let text = recorded(&files)?;
    for manifest in ["Cargo.toml", "core/Cargo.toml", "tool/Cargo.toml"] {
        let mut edited = files.clone();
        edited.files.insert(path(manifest)?, b"# edited\n".to_vec());
        assert_eq!(
            recorded_metadata(&edited, Some(text.as_bytes())),
            Err(MetadataFileError::Changed(path(manifest)?)),
            "{manifest}"
        );
        let mut deleted = files.clone();
        deleted.files.remove(&path(manifest)?);
        assert_eq!(
            recorded_metadata(&deleted, Some(text.as_bytes())),
            Err(MetadataFileError::Changed(path(manifest)?)),
            "{manifest} deleted"
        );
    }
    Ok(())
}

#[test]
fn a_manifest_the_file_does_not_list_is_named_unless_the_config_excludes_it() -> TestResult {
    let files = two_workspaces()?;
    let text = recorded(&files)?;
    let mut grown = files.clone();
    grown.tracked_paths.insert(path("core/extra/Cargo.toml")?);
    grown
        .files
        .insert(path("core/extra/Cargo.toml")?, b"# new\n".to_vec());
    assert_eq!(
        recorded_metadata(&grown, Some(text.as_bytes())),
        Err(MetadataFileError::Unlisted(path("core/extra/Cargo.toml")?))
    );
    let config = RepoPath::new(".velnor/rust-ci.toml")?;
    grown.tracked_paths.insert(config.clone());
    grown
        .files
        .insert(config, b"exclude = [\"core/extra\"]\n".to_vec());
    assert_eq!(
        recorded_metadata(&grown, Some(text.as_bytes()))?,
        files.rust_metadata
    );
    Ok(())
}

#[test]
fn a_missing_or_unreadable_file_asks_for_generate() -> TestResult {
    let files = two_workspaces()?;
    let text = recorded(&files)?;
    let skewed = [
        "not json".to_owned(),
        text.replace("\"tests\"", "\"tested\""),
        text.replace("\"lib\"", "\"cdylib2\""),
        text.replace("\"normal\"", "\"sideways\""),
    ];
    assert_eq!(
        recorded_metadata(&files, None),
        Err(MetadataFileError::Missing)
    );
    for skew in &skewed {
        let error = recorded_metadata(&files, Some(skew.as_bytes()))
            .err()
            .ok_or("accepted")?;
        assert!(matches!(error, MetadataFileError::Unreadable(_)), "{skew}");
        assert!(error.to_string().ends_with("run velnor-gen generate"));
    }
    Ok(())
}

#[test]
fn generate_writes_the_file_and_drift_reports_it_missing_changed_or_stale() -> TestResult {
    let files = two_workspaces()?;
    let generated = generated_files(&files)?;
    assert_eq!(
        generated.keys().cloned().collect::<Vec<_>>(),
        [
            velnor_render::DELTA_CLEANUP_PATH,
            path(".github/workflows/rust-ci.yml")?,
            METADATA_PATH,
        ]
    );
    let on_disk = generated
        .iter()
        .map(|(path, text)| (path.clone(), text.clone().into_bytes()))
        .collect();
    assert_eq!(drift(&generated, &on_disk, false), []);
    let mut changed = on_disk.clone();
    changed.insert(METADATA_PATH, b"{}".to_vec());
    assert_eq!(
        drift(&generated, &changed, false),
        [Drift::Changed(METADATA_PATH)]
    );
    let mut missing = on_disk;
    missing.remove(&METADATA_PATH);
    assert_eq!(
        drift(&generated, &missing, false),
        [Drift::Missing(METADATA_PATH)]
    );
    Ok(())
}

#[test]
fn a_file_cargo_would_discover_as_a_target_after_the_file_was_written_is_refused() -> TestResult {
    let files = two_workspaces()?;
    let text = recorded(&files)?;
    for added in [
        "core/src/lib.rs",
        "core/src/main.rs",
        "core/src/bin/tool.rs",
        "core/src/bin/tool/main.rs",
        "core/tests/it.rs",
        "core/examples/demo/main.rs",
        "core/benches/speed.rs",
        "core/build.rs",
        "src/main.rs",
    ] {
        let mut grown = files.clone();
        grown.tracked_paths.insert(path(added)?);
        assert_eq!(
            recorded_metadata(&grown, Some(text.as_bytes())),
            Err(MetadataFileError::Layout),
            "{added}"
        );
    }
    for added in [
        "core/tests/common/mod.rs",
        "core/src/util.rs",
        "docs/example.rs",
        "core/readme.md",
    ] {
        let mut grown = files.clone();
        grown.tracked_paths.insert(path(added)?);
        assert_eq!(
            recorded_metadata(&grown, Some(text.as_bytes()))?,
            files.rust_metadata,
            "{added}"
        );
    }
    Ok(())
}

#[test]
fn a_manifest_without_a_digest_in_the_file_is_refused() -> TestResult {
    let files = two_workspaces()?;
    let text = recorded(&files)?;
    for manifest in ["Cargo.toml", "core/Cargo.toml", "tool/Cargo.toml"] {
        let mut value: serde_json::Value = serde_json::from_str(&text)?;
        value["manifests"]
            .as_object_mut()
            .ok_or("manifests")?
            .remove(manifest);
        assert_eq!(
            recorded_metadata(&files, Some(value.to_string().as_bytes())),
            Err(MetadataFileError::Unlisted(path(manifest)?)),
            "{manifest}"
        );
    }
    Ok(())
}

#[test]
fn only_a_rust_file_in_a_discovered_folder_changes_the_recorded_layout() -> TestResult {
    let files = two_workspaces()?;
    let text = recorded(&files)?;
    let mut notes = files.clone();
    notes.tracked_paths.insert(path("core/tests/notes.txt")?);
    notes
        .files
        .insert(path("core/tests/notes.txt")?, b"notes\n".to_vec());
    assert_eq!(
        recorded_metadata(&notes, Some(text.as_bytes()))?,
        files.rust_metadata
    );
    let mut extra = files.clone();
    extra.tracked_paths.insert(path("core/tests/extra.rs")?);
    extra
        .files
        .insert(path("core/tests/extra.rs")?, b"fn main() {}\n".to_vec());
    assert_eq!(
        recorded_metadata(&extra, Some(text.as_bytes())),
        Err(MetadataFileError::Layout)
    );
    Ok(())
}

#[test]
fn a_file_written_before_versions_were_recorded_is_refused_with_the_regenerate_command()
-> TestResult {
    let files = two_workspaces()?;
    let mut json: serde_json::Value = serde_json::from_str(&recorded(&files)?)?;
    for root in json["roots"].as_object_mut().ok_or("roots")?.values_mut() {
        for member in root["members"].as_array_mut().ok_or("members")? {
            member.as_object_mut().ok_or("member")?.remove("version");
        }
    }
    let old = serde_json::to_string(&json)?;
    assert!(!old.contains("\"version\""));
    let Err(error) = recorded_metadata(&files, Some(old.as_bytes())) else {
        return Err("an old-format file was accepted".into());
    };
    assert!(matches!(error, MetadataFileError::Unreadable(_)));
    let message = error.to_string();
    assert!(message.contains(".velnor/rust-metadata.json"), "{message}");
    assert!(message.contains("version"), "{message}");
    assert!(message.ends_with("run velnor-gen generate"), "{message}");
    Ok(())
}

#[test]
fn a_pin_below_0_1_81_gets_a_file_its_helper_can_read() -> TestResult {
    let mut files = two_workspaces()?;
    pin(&mut files, "0.1.79");
    files
        .tracked_paths
        .insert(RepoPath::new(".velnor/rust-ci.toml")?);
    files.files.insert(
        RepoPath::new(".velnor/rust-ci.toml")?,
        b"full_run = \"off\"\n".to_vec(),
    );
    let old = recorded(&files)?;
    assert!(!old.contains("\"version\""), "{old}");
    assert!(generated_files(&files).is_ok());
    let read = recorded_metadata(&files, Some(old.as_bytes()))?;
    assert!(
        read.values()
            .flat_map(|workspace| &workspace.members)
            .all(|member| member.version.is_none())
    );
    pin(&mut files, "0.1.81");
    assert!(recorded(&files)?.contains("\"version\""));
    Ok(())
}

#[test]
fn the_pin_decides_whether_a_recorded_version_is_read() -> TestResult {
    let files = two_workspaces()?;
    let new = recorded(&files)?;
    let mut old_pin = files.clone();
    pin(&mut old_pin, "0.1.79");
    let read = recorded_metadata(&old_pin, Some(new.as_bytes()))?;
    assert!(
        read.values()
            .flat_map(|workspace| &workspace.members)
            .all(|member| member.version.is_none())
    );
    let old = recorded(&old_pin)?;
    assert!(matches!(
        recorded_metadata(&files, Some(old.as_bytes())),
        Err(MetadataFileError::Unreadable(_))
    ));
    Ok(())
}

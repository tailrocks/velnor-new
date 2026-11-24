// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::io::TestResult;
use std::collections::BTreeMap;
use std::path::Path;
use velnor_gen::io::{SAMPLE_REPOSITORY, recording_of, relocate_metadata};
use velnor_model::RepoPath;

#[test]
fn canonical_windows_checkout_relocates_cargo_paths() -> TestResult {
    let json = br#"{"manifest":"C:\\work\\repo\\crate\\Cargo.toml","id":"path+file:///C:/work/repo/crate#0.1.0","neighbor":"C:\\work\\repository\\crate"}"#;
    let value: serde_json::Value =
        serde_json::from_slice(&relocate_metadata(Path::new(r"\\?\C:\work\repo"), json)?)?;
    assert_eq!(value["manifest"], "/repo/crate/Cargo.toml");
    assert_eq!(value["id"], "path+file:///repo/crate#0.1.0");
    assert_eq!(value["neighbor"], r"C:\work\repository\crate");
    Ok(())
}

#[test]
fn checkout_path_in_an_option_is_relocated() -> TestResult {
    let json = br#"{"flag":"--manifest-path=/tmp/repo/Cargo.toml","neighbor":"--manifest-path=/tmp/repository/Cargo.toml"}"#;
    let value: serde_json::Value =
        serde_json::from_slice(&relocate_metadata(Path::new("/tmp/repo"), json)?)?;
    assert_eq!(value["flag"], "--manifest-path=/repo/Cargo.toml");
    assert_eq!(
        value["neighbor"],
        "--manifest-path=/tmp/repository/Cargo.toml"
    );
    Ok(())
}

#[test]
fn canonical_unc_checkout_relocates_both_path_spellings() -> TestResult {
    let json = br#"{"plain":"\\\\server\\share\\repo\\Cargo.toml","verbatim":"\\\\?\\UNC\\server\\share\\repo\\Cargo.toml"}"#;
    let value: serde_json::Value = serde_json::from_slice(&relocate_metadata(
        Path::new(r"\\?\UNC\server\share\repo"),
        json,
    )?)?;
    assert_eq!(value["plain"], "/repo/Cargo.toml");
    assert_eq!(value["verbatim"], "/repo/Cargo.toml");
    Ok(())
}

#[test]
fn canonical_unc_checkout_relocates_the_package_id_authority_form() -> TestResult {
    let json = br#"{"id":"path+file://server/share/repo/crate#0.1.0","manifest":"\\\\server\\share\\repo\\crate\\Cargo.toml"}"#;
    let value: serde_json::Value = serde_json::from_slice(&relocate_metadata(
        Path::new(r"\\?\UNC\server\share\repo"),
        json,
    )?)?;
    assert_eq!(value["id"], "path+file:///repo/crate#0.1.0");
    assert_eq!(value["manifest"], "/repo/crate/Cargo.toml");
    Ok(())
}

#[test]
fn a_colon_separated_path_list_relocates_every_path() -> TestResult {
    let json = br#"{"paths":"/tmp/repo/bin:/tmp/repo/tools"}"#;
    let value: serde_json::Value =
        serde_json::from_slice(&relocate_metadata(Path::new("/tmp/repo"), json)?)?;
    assert_eq!(value["paths"], "/repo/bin:/repo/tools");
    Ok(())
}

#[test]
fn a_colon_separated_windows_path_list_relocates_every_path() -> TestResult {
    let json = br#"{"paths":"C:\\work\\repo\\bin:C:\\work\\repo\\tools"}"#;
    let value: serde_json::Value =
        serde_json::from_slice(&relocate_metadata(Path::new(r"C:\work\repo"), json)?)?;
    assert_eq!(value["paths"], "/repo/bin:/repo/tools");
    Ok(())
}

#[test]
fn a_colon_inside_a_file_name_is_not_a_path_separator() -> TestResult {
    let json = br#"{"path":"/tmp/repo/dir:name/file"}"#;
    let value: serde_json::Value =
        serde_json::from_slice(&relocate_metadata(Path::new("/tmp/repo"), json)?)?;
    assert_eq!(value["path"], "/repo/dir:name/file");
    let json = br#"{"path":"C:\\work\\repo\\dir:name"}"#;
    let value: serde_json::Value =
        serde_json::from_slice(&relocate_metadata(Path::new(r"C:\work\repo"), json)?)?;
    assert_eq!(value["path"], "/repo/dir:name");
    Ok(())
}

#[test]
fn recording_a_repository_never_records_its_own_samples() -> TestResult {
    let sample = crate::golden::samples().join("single-crate");
    let mut view = crate::golden::load(&sample)?;
    let nested = RepoPath::new("generator/samples/old/view/Cargo.toml")?;
    view.tracked_paths.insert(nested.clone());
    view.files
        .insert(nested, b"[package]\nname = \"nested\"\n".to_vec());
    let recording = recording_of(
        Path::new(SAMPLE_REPOSITORY),
        view,
        BTreeMap::new(),
        &mut Vec::new(),
    )?;
    let recorded = |path: &RepoPath| path.as_str().starts_with("generator/samples/");
    assert!(!recording.view.keys().any(recorded), "{:?}", recording.view);
    assert!(
        !recording.manifests.iter().any(recorded),
        "{:?}",
        recording.manifests
    );
    Ok(())
}

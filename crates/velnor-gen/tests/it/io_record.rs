// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;
#[cfg(target_os = "macos")]
use std::process::Command;

use velnor_gen::io::{
    Error as IoError, Recording, SAMPLE_REPOSITORY, SampleWriteOutcome, recording_of,
    relocate_metadata, write_sample,
};
use velnor_model::RepoPath;

use super::io::{Scratch, TestResult, write};

fn files(entries: &[(&str, &str)]) -> Result<BTreeMap<RepoPath, Vec<u8>>, Box<dyn Error>> {
    entries
        .iter()
        .map(|(path, text)| Ok((RepoPath::new(*path)?, text.as_bytes().to_vec())))
        .collect()
}

#[test]
fn recording_a_sample_replaces_the_recorded_inputs_and_keeps_the_expected_files() -> TestResult {
    let scratch = Scratch::new("record")?;
    let samples = scratch.repo().join("generator/samples");
    let sample = scratch.repo().join("generator/samples/demo");
    let expected = sample.join("expected");
    write(
        &sample.join("expected/.github/workflows/rust-ci.yml"),
        "kept\n",
    )?;
    symlink(".github/workflows/rust-ci.yml", expected.join("link"))?;
    write(&sample.join("view/old/Cargo.toml"), "")?;
    write(&sample.join("cargo-metadata/old.json"), "{}")?;
    fs::set_permissions(&expected, fs::Permissions::from_mode(0o555))?;
    let result = write_sample(
        &scratch.repo(),
        "demo",
        &Recording {
            view: files(&[("mise.toml", "[tools]\n")])?,
            manifests: vec![
                RepoPath::new("Cargo.toml")?,
                RepoPath::new("lab/Cargo.toml")?,
            ],
            symlinks: vec![RepoPath::new(".cargo")?],
            metadata: files(&[("lab/Cargo.toml", "{}\n")])?,
        },
    );
    fs::set_permissions(&expected, fs::Permissions::from_mode(0o755))?;
    for entry in fs::read_dir(&samples)? {
        let expected = entry?.path().join("previous/expected");
        if expected.exists() {
            fs::set_permissions(expected, fs::Permissions::from_mode(0o755))?;
        }
    }
    assert_eq!(result?, SampleWriteOutcome::Recorded);
    for (path, bytes) in [
        ("manifests.txt", "Cargo.toml\nlab/Cargo.toml\n"),
        ("symlinks.txt", ".cargo\n"),
        ("view/mise.toml", "[tools]\n"),
        ("cargo-metadata/lab/Cargo.toml.json", "{}\n"),
        ("expected/.github/workflows/rust-ci.yml", "kept\n"),
    ] {
        assert_eq!(fs::read_to_string(sample.join(path))?, bytes, "{path}");
    }
    assert!(!sample.join("view/old").exists());
    assert!(!sample.join("cargo-metadata/old.json").exists());
    assert_eq!(
        fs::read_link(sample.join("expected/link"))?,
        Path::new(".github/workflows/rust-ci.yml")
    );
    assert_eq!(fs::read_dir(&samples)?.count(), 1);
    Ok(())
}

#[test]
fn recording_a_sample_preserves_symlinks_from_the_repository_view() -> TestResult {
    let scratch = Scratch::new("record-symlink")?;
    let sample = crate::golden::samples().join("single-crate");
    let mut view = crate::golden::load(&sample)?;
    view.symlinks.insert(RepoPath::new("README.md")?);
    let metadata = BTreeMap::from([(
        RepoPath::new("Cargo.toml")?,
        fs::read(sample.join("cargo-metadata/Cargo.toml.json"))?,
    )]);
    let recording = recording_of(
        Path::new(SAMPLE_REPOSITORY),
        view,
        metadata,
        &mut Vec::new(),
    )?;
    write_sample(&scratch.repo(), "demo", &recording)?;
    let recorded = crate::golden::load(&scratch.repo().join("generator/samples/demo"))?;
    assert!(recorded.symlinks.contains(&RepoPath::new("README.md")?));
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn a_published_sample_reports_failure_to_remove_its_previous_version() -> TestResult {
    let scratch = WorktreeScratch::new()?;
    let sample = scratch.repo().join("generator/samples/demo");
    let obsolete = sample.join("view/obsolete.txt");
    write(&obsolete, "old\n")?;
    let immutable = ImmutableTree::new(scratch.repo().join("generator"), &obsolete);
    let result = write_sample(
        &scratch.repo(),
        "demo",
        &Recording {
            view: files(&[("mise.toml", "[tools]\n")])?,
            ..Recording::default()
        },
    )?;
    let SampleWriteOutcome::RecordedWithCleanupFailure {
        staging_path,
        error,
    } = result
    else {
        return Err("published sample did not report cleanup failure".into());
    };
    assert_ne!(error, "");
    assert!(staging_path.is_dir());
    assert_eq!(
        staging_path.parent(),
        Some(scratch.repo().join("generator/samples").as_path())
    );

    assert_eq!(
        fs::read_to_string(sample.join("view/mise.toml"))?,
        "[tools]\n"
    );
    assert!(!sample.join("view/obsolete.txt").exists());
    assert_eq!(
        fs::read_to_string(staging_path.join("previous/view/obsolete.txt"))?,
        "old\n"
    );
    drop(immutable);
    Ok(())
}

#[cfg(target_os = "macos")]
struct ImmutableTree(std::path::PathBuf);

#[cfg(target_os = "macos")]
impl ImmutableTree {
    fn new(root: std::path::PathBuf, file: &Path) -> Self {
        let status = Command::new("/usr/bin/chflags")
            .arg("uchg")
            .arg(file)
            .status();
        assert!(
            status.as_ref().is_ok_and(std::process::ExitStatus::success),
            "{status:?}"
        );
        Self(root)
    }
}

#[cfg(target_os = "macos")]
impl Drop for ImmutableTree {
    fn drop(&mut self) {
        Command::new("/usr/bin/chflags")
            .arg("-R")
            .arg("nouchg")
            .arg(&self.0)
            .status()
            .ok();
    }
}

#[cfg(target_os = "macos")]
struct WorktreeScratch(std::path::PathBuf);

#[cfg(target_os = "macos")]
impl WorktreeScratch {
    fn new() -> Result<Self, Box<dyn Error>> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::current_dir()?
            .join(".tmp-cleanup-tests")
            .join(format!(
                "{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn repo(&self) -> std::path::PathBuf {
        self.0.join("repo")
    }
}

#[cfg(target_os = "macos")]
impl Drop for WorktreeScratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

#[test]
fn recording_a_sample_never_writes_or_removes_through_a_symbolic_link() -> TestResult {
    let inputs = Recording {
        view: files(&[("mise.toml", "")])?,
        ..Recording::default()
    };

    let directory_link = Scratch::new("record-directory-link")?;
    write(&directory_link.outside().join("keep.txt"), "theirs\n")?;
    directory_link.link("generator/samples/demo/view", &directory_link.outside())?;
    let result = write_sample(&directory_link.repo(), "demo", &inputs);
    assert!(matches!(result, Err(IoError::NotRegular(_))), "{result:?}");
    assert!(directory_link.outside().join("keep.txt").exists());
    assert!(!directory_link.outside().join("mise.toml").exists());
    let result = write_sample(&directory_link.repo(), "demo", &Recording::default());
    assert!(matches!(result, Err(IoError::NotRegular(_))), "{result:?}");
    assert!(directory_link.outside().join("keep.txt").exists());

    let samples_link = Scratch::new("record-samples-link")?;
    samples_link.link("generator/samples", &samples_link.outside())?;
    let result = write_sample(&samples_link.repo(), "demo", &inputs);
    assert!(matches!(result, Err(IoError::NotRegular(_))), "{result:?}");
    assert_eq!(fs::read_dir(samples_link.outside())?.count(), 0);

    let file_link = Scratch::new("record-file-link")?;
    let target = file_link.outside().join("victim");
    write(&target, "theirs\n")?;
    file_link.link("generator/samples/demo/view/mise.toml", &target)?;
    let result = write_sample(&file_link.repo(), "demo", &inputs);
    assert!(matches!(result, Err(IoError::NotRegular(_))), "{result:?}");
    assert_eq!(fs::read_to_string(&target)?, "theirs\n");
    Ok(())
}

#[test]
fn a_sample_name_cannot_leave_the_samples_folder() -> TestResult {
    let scratch = Scratch::new("record-name")?;
    for name in ["", "../x", "a/b", "A", "-x", ".."] {
        let result = write_sample(&scratch.repo(), name, &Recording::default());
        assert!(
            matches!(result, Err(IoError::Repository(_))),
            "{name}: {result:?}"
        );
    }
    assert!(!scratch.repo().join("generator").exists());
    Ok(())
}

#[test]
fn recorded_metadata_no_longer_names_the_checkout_it_came_from() -> TestResult {
    let checkout = Path::new("/home/someone/work/project");
    let json = br#"{"workspace_root":"/home/someone/work/project","packages":[{"name":"a","version":"0.1.0","id":"path+file:///home/someone/work/project/crates/a#0.1.0","manifest_path":"/home/someone/work/project/crates/a/Cargo.toml","dependencies":[],"targets":[]}]}"#;
    let recorded = relocate_metadata(checkout, json)?;
    assert!(!String::from_utf8(recorded.clone())?.contains("someone"));
    let (workspace, metadata) =
        velnor_rust::reduce_metadata(Path::new(SAMPLE_REPOSITORY), &recorded)?;
    assert_eq!(workspace, RepoPath::new("Cargo.toml")?);
    assert_eq!(
        metadata.members[0].manifest,
        RepoPath::new("crates/a/Cargo.toml")?
    );
    Ok(())
}

#[test]
fn recorded_metadata_names_no_checkout_whose_path_needs_url_escaping() -> TestResult {
    let json = br#"{"workspace_root":"/home/some one/project name","packages":[{"name":"a","version":"0.1.0","id":"path+file:///home/some%20one/project%20name/crates/a#0.1.0","manifest_path":"/home/some one/project name/crates/a/Cargo.toml","dependencies":[],"targets":[]}]}"#;
    let recorded = relocate_metadata(Path::new("/home/some one/project name"), json)?;
    let text = String::from_utf8(recorded)?;
    assert!(!text.contains("some"), "{text}");
    assert!(!text.contains("project"), "{text}");
    Ok(())
}

#[test]
fn a_rejected_recording_removes_nothing_from_the_sample() -> TestResult {
    let scratch = Scratch::new("record-atomic")?;
    let sample = scratch.repo().join("generator/samples/demo");
    write(&sample.join("view/mise.toml"), "kept\n")?;
    scratch.link("generator/samples/demo/cargo-metadata", &scratch.outside())?;
    let result = write_sample(&scratch.repo(), "demo", &Recording::default());
    assert!(matches!(result, Err(IoError::NotRegular(_))), "{result:?}");
    assert_eq!(fs::read_to_string(sample.join("view/mise.toml"))?, "kept\n");
    Ok(())
}

#[test]
fn a_failed_recording_keeps_every_existing_sample_file_unchanged() -> TestResult {
    let scratch = Scratch::new("record-failed-write")?;
    let sample = scratch.repo().join("generator/samples/demo");
    let kept = [
        ("view/mise.toml", "kept\n"),
        ("cargo-metadata/root.json", "metadata\n"),
        ("manifests.txt", "old/Cargo.toml\n"),
        ("symlinks.txt", "old-link\n"),
        ("expected/.github/workflows/rust-ci.yml", "expected\n"),
    ];
    for (path, text) in kept {
        write(&sample.join(path), text)?;
    }
    let long_path = format!("new/{}", "a".repeat(300));
    let recording = Recording {
        view: files(&[(&long_path, "unwritable")])?,
        manifests: vec![RepoPath::new("Cargo.toml")?],
        symlinks: vec![RepoPath::new(".cargo")?],
        ..Recording::default()
    };
    assert!(write_sample(&scratch.repo(), "demo", &recording).is_err());
    for (path, text) in kept {
        assert_eq!(fs::read_to_string(sample.join(path))?, text, "{path}");
    }
    Ok(())
}

#[test]
fn a_checkout_at_the_filesystem_root_cannot_be_recorded() {
    for checkout in [Path::new("/"), Path::new(r"C:\"), Path::new("C:/")] {
        let result = relocate_metadata(checkout, b"{}");
        assert!(
            matches!(result, Err(IoError::Repository(_))),
            "{checkout:?}: {result:?}"
        );
    }
}

#[test]
fn recorded_metadata_names_no_checkout_in_object_keys() -> TestResult {
    let json = br#"{"metadata":{"/home/someone/work/project/x":1}}"#;
    let recorded = relocate_metadata(Path::new("/home/someone/work/project"), json)?;
    assert!(!String::from_utf8(recorded)?.contains("someone"));
    Ok(())
}

#[test]
fn recorded_metadata_relocates_only_checkout_path_prefixes() -> TestResult {
    let json = br#"{"nearby":"/Users/me/workspace/crate","manifest":"/rep/crates/a/Cargo.toml"}"#;
    let recorded = relocate_metadata(Path::new("/rep"), json)?;
    let value: serde_json::Value = serde_json::from_slice(&recorded)?;
    assert_eq!(value["manifest"], "/repo/crates/a/Cargo.toml");
    let recorded = relocate_metadata(Path::new("/Users/me/work"), json)?;
    let value: serde_json::Value = serde_json::from_slice(&recorded)?;
    assert_eq!(value["nearby"], "/Users/me/workspace/crate");
    let json = br#"["/tmp/repo#other/crate","/tmp/repo?other/crate","path+file:///tmp/repo#package","file:///tmp/repo?query"]"#;
    let recorded = relocate_metadata(Path::new("/tmp/repo"), json)?;
    let value: serde_json::Value = serde_json::from_slice(&recorded)?;
    assert_eq!(value[0], "/tmp/repo#other/crate");
    assert_eq!(value[1], "/tmp/repo?other/crate");
    assert_eq!(value[2], "path+file:///repo#package");
    assert_eq!(value[3], "file:///repo?query");
    Ok(())
}

#[test]
fn recorded_metadata_relocates_every_checkout_path_in_a_string() -> TestResult {
    let json = br#"{"dependencies":"path+file:///tmp/repo%20work/crates/a#0.1.0; path+file:///tmp/repo%20work/crates/b#0.1.0; path+file:///tmp/repository/crates/c#0.1.0"}"#;
    let recorded = relocate_metadata(Path::new("/tmp/repo work"), json)?;
    let value: serde_json::Value = serde_json::from_slice(&recorded)?;
    assert_eq!(
        value["dependencies"],
        "path+file:///repo/crates/a#0.1.0; path+file:///repo/crates/b#0.1.0; path+file:///tmp/repository/crates/c#0.1.0"
    );
    Ok(())
}

#[test]
fn recorded_metadata_relocates_windows_checkout_paths() -> TestResult {
    let json = br#"{"manifest":"C:\\work\\repo\\crate\\Cargo.toml","id":"path+file:///C:/work/repo/crate#0.1.0","neighbor":"C:\\work\\repository\\crate\\Cargo.toml"}"#;
    let recorded = relocate_metadata(Path::new(r"C:\work\repo"), json)?;
    let value: serde_json::Value = serde_json::from_slice(&recorded)?;
    assert_eq!(value["manifest"], "/repo/crate/Cargo.toml");
    assert_eq!(value["id"], "path+file:///repo/crate#0.1.0");
    assert_eq!(value["neighbor"], r"C:\work\repository\crate\Cargo.toml");
    Ok(())
}

#[test]
fn recorded_metadata_rejects_keys_that_collapse_during_relocation() {
    let json = br#"{"/tmp/repo name/a":1,"/tmp/repo%20name/a":2}"#;
    let result = relocate_metadata(Path::new("/tmp/repo name"), json);
    assert!(matches!(result, Err(IoError::Repository(_))), "{result:?}");
}

#[test]
fn recording_a_repository_never_records_its_own_samples() -> TestResult {
    let sample = crate::golden::samples().join("single-crate");
    let mut view = crate::golden::load(&sample)?;
    let nested = "generator/samples/old/view/Cargo.toml";
    view.tracked_paths.insert(RepoPath::new(nested)?);
    view.tracked_paths
        .insert(RepoPath::new("generator/samples/old/manifests.txt")?);
    view.files
        .insert(RepoPath::new(nested)?, b"[package]\nname = \"nested\"\n".to_vec());
    let recording = recording_of(
        Path::new(SAMPLE_REPOSITORY),
        view,
        BTreeMap::new(),
        &mut Vec::new(),
    )?;
    assert!(
        recording
            .view
            .keys()
            .all(|path| !path.as_str().starts_with("generator/samples/")),
        "{:?}",
        recording.view.keys().collect::<Vec<_>>()
    );
    assert!(
        recording
            .manifests
            .iter()
            .all(|path| !path.as_str().starts_with("generator/samples/")),
        "{:?}",
        recording.manifests
    );
    Ok(())
}

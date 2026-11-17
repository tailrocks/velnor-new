// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use velnor_gen::io::recording_of;

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
    assert_eq!(
        fs::read_to_string(sample.join("manifests.txt"))?,
        "Cargo.toml\nlab/Cargo.toml\n"
    );
    assert_eq!(fs::read_to_string(sample.join("symlinks.txt"))?, ".cargo\n");
    assert_eq!(
        fs::read_to_string(sample.join("view/mise.toml"))?,
        "[tools]\n"
    );
    assert_eq!(
        fs::read_to_string(sample.join("cargo-metadata/lab/Cargo.toml.json"))?,
        "{}\n"
    );
    fs::set_permissions(&expected, fs::Permissions::from_mode(0o755))?;
    for entry in fs::read_dir(&samples)? {
        let expected = entry?.path().join("previous/expected");
        if expected.exists() {
            fs::set_permissions(expected, fs::Permissions::from_mode(0o755))?;
        }
    }
    result?;
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

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs, os::unix::fs::PermissionsExt};

use super::bounded::copy_oracle;
use super::support::{Harness, diagnostics_candidate, retained_workspace};

const GOLDEN_TREE_LIMIT: u64 = 64 * 1024 * 1024;

#[test]
fn bounds_a_single_golden_file_before_reading_it() -> Result<(), Box<dyn Error>> {
    let harness = Harness::new()?;
    let candidate = diagnostics_candidate()?;
    let source = harness.root.join("docs/proposed/opentofu-goldens");
    let oracle = harness.directory.join("single-file-limit-oracle");
    copy_oracle(&source, &oracle)?;
    let oversized = oracle.join("cases/nested/oversized.bin");
    fs::File::create(&oversized)?.set_len(GOLDEN_TREE_LIMIT + 1)?;
    let manifest_before = fs::read(oracle.join("MANIFEST.sha256"))?;
    let output = harness.run(
        "check",
        &harness.root,
        &candidate,
        &harness.archive_guard,
        &harness.guard_sha256,
        Some(&oracle),
    )?;

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("tree exceeds its byte limit"));
    assert_eq!(fs::read(oracle.join("MANIFEST.sha256"))?, manifest_before);
    assert_eq!(fs::metadata(oversized)?.len(), GOLDEN_TREE_LIMIT + 1);
    Ok(())
}

#[test]
fn bounds_the_combined_golden_tree_size() -> Result<(), Box<dyn Error>> {
    let harness = Harness::new()?;
    let candidate = diagnostics_candidate()?;
    let source = harness.root.join("docs/proposed/opentofu-goldens");
    let oracle = harness.directory.join("tree-limit-oracle");
    copy_oracle(&source, &oracle)?;
    for name in ["first.bin", "second.bin"] {
        fs::File::create(oracle.join("cases/nested").join(name))?.set_len(33 * 1024 * 1024)?;
    }
    let manifest_before = fs::read(oracle.join("MANIFEST.sha256"))?;
    let output = harness.run(
        "check",
        &harness.root,
        &candidate,
        &harness.archive_guard,
        &harness.guard_sha256,
        Some(&oracle),
    )?;

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("tree exceeds its byte limit"));
    assert_eq!(fs::read(oracle.join("MANIFEST.sha256"))?, manifest_before);
    Ok(())
}

#[test]
fn rejects_a_golden_file_whose_executable_mode_changed() -> Result<(), Box<dyn Error>> {
    let harness = Harness::new()?;
    let candidate = diagnostics_candidate()?;
    let source = harness.root.join("docs/proposed/opentofu-goldens");
    let oracle = harness.directory.join("mode-change-oracle");
    copy_oracle(&source, &oracle)?;
    let changed = oracle.join("cases/nested/plan.txt");
    let source_mode = fs::metadata(source.join("cases/nested/plan.txt"))?
        .permissions()
        .mode()
        & 0o111;
    let original_mode = fs::metadata(&changed)?.permissions().mode();
    assert_eq!(original_mode & 0o111, source_mode);
    let mut permissions = fs::metadata(&changed)?.permissions();
    permissions.set_mode(original_mode | 0o100);
    fs::set_permissions(&changed, permissions)?;
    let manifest = oracle.join("MANIFEST.sha256");
    let manifest_text = fs::read_to_string(&manifest)?;
    let manifest_line = manifest_text
        .lines()
        .find(|line| line.ends_with("./nested/plan.txt"))
        .ok_or("changed golden file is not in the manifest")?;
    assert!(manifest_line.starts_with("- "));
    let updated_line = manifest_line.replacen("- ", "x ", 1);
    let manifest_before = manifest_text.replacen(manifest_line, &updated_line, 1);
    fs::write(&manifest, &manifest_before)?;
    let manifest_before = fs::read(&manifest)?;
    let output = harness.run(
        "check",
        &harness.root,
        &candidate,
        &harness.archive_guard,
        &harness.guard_sha256,
        Some(&oracle),
    )?;

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("golden mismatch: nested"), "{stderr}");
    assert!(!stderr.contains("golden manifest"), "{stderr}");
    assert_eq!(fs::read(&manifest)?, manifest_before);
    assert_ne!(fs::metadata(changed)?.permissions().mode() & 0o111, 0);
    Ok(())
}

#[test]
fn rejects_an_invalid_manifest_before_capture_without_changing_cases() -> Result<(), Box<dyn Error>>
{
    let harness = Harness::new()?;
    let candidate = diagnostics_candidate()?;
    let source = harness.root.join("docs/proposed/opentofu-goldens");
    let oracle = harness.directory.join("failed-publication-oracle");
    copy_oracle(&source, &oracle)?;
    let retained = oracle.join("retained-executable");
    fs::write(&retained, b"retained\n")?;
    let mut retained_permissions = fs::metadata(&retained)?.permissions();
    retained_permissions.set_mode(0o755);
    fs::set_permissions(&retained, retained_permissions)?;
    let successful_capture = harness.run(
        "capture",
        &harness.root,
        &candidate,
        &harness.archive_guard,
        &harness.guard_sha256,
        Some(&oracle),
    )?;
    assert!(
        successful_capture.status.success(),
        "successful capture failed: {}",
        String::from_utf8_lossy(&successful_capture.stderr)
    );
    assert_eq!(fs::read(&retained)?, b"retained\n");
    assert_ne!(fs::metadata(&retained)?.permissions().mode() & 0o111, 0);
    let checked = harness.run(
        "check",
        &harness.root,
        &candidate,
        &harness.archive_guard,
        &harness.guard_sha256,
        Some(&oracle),
    )?;
    assert!(
        checked.status.success(),
        "captured oracle failed check: {}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let original_plan = fs::read(oracle.join("cases/nested/plan.stderr.txt"))?;
    fs::remove_file(oracle.join("MANIFEST.sha256"))?;
    fs::create_dir(oracle.join("MANIFEST.sha256"))?;
    let mut command = harness.command(
        "capture",
        &harness.root,
        &candidate,
        &harness.archive_guard,
        &harness.guard_sha256,
        Some(&oracle),
    );
    command
        .env("VELNOR_PLAN_DIAGNOSTICS_MODE", "warning-all")
        .env("VELNOR_PLAN_DIAGNOSTICS_ROOT", &harness.root);
    let output = command.output()?;

    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("golden manifest must be a regular non-symlink file")
    );
    assert!(oracle.join("MANIFEST.sha256").is_dir());
    assert_eq!(
        fs::read(oracle.join("cases/nested/plan.stderr.txt"))?,
        original_plan
    );
    assert!(retained_workspace(&output).is_none());
    Ok(())
}

#[test]
fn rejects_excessive_oracle_nesting_before_candidate_execution() -> Result<(), Box<dyn Error>> {
    let harness = Harness::new()?;
    let oracle = harness.directory.join("deep-oracle");
    copy_oracle(
        &harness.root.join("docs/proposed/opentofu-goldens"),
        &oracle,
    )?;
    let mut path = oracle.join("extra");
    fs::create_dir(&path)?;
    for _ in 0..127 {
        path.push("d");
        fs::create_dir(&path)?;
    }
    let accepted = harness.run(
        "check",
        &harness.root,
        &harness.guard_source,
        &harness.archive_guard,
        &harness.guard_sha256,
        Some(&oracle),
    )?;
    assert_version_failure(&accepted);
    path.push("d");
    fs::create_dir(&path)?;
    let output = harness.run(
        "check",
        &harness.root,
        &harness.guard_source,
        &harness.archive_guard,
        &harness.guard_sha256,
        Some(&oracle),
    )?;
    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("tree exceeds its structural limit"));
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("candidate --version command failed")
    );
    Ok(())
}

fn tree_usage(root: &std::path::Path) -> Result<(usize, usize), Box<dyn Error>> {
    use std::os::unix::ffi::OsStrExt;
    let mut pending = vec![root.to_path_buf()];
    let mut count = 0;
    let mut bytes = 0;
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            count += 1;
            bytes += path.strip_prefix(root)?.as_os_str().as_bytes().len();
            if entry.file_type()?.is_dir() {
                pending.push(path);
            }
        }
    }
    Ok((count, bytes))
}

#[test]
fn accepts_exact_oracle_entry_and_path_byte_limits_and_rejects_one_over()
-> Result<(), Box<dyn Error>> {
    const MAX_ENTRIES: usize = 20_000;
    const MAX_PATH_BYTES: usize = 4 * 1024 * 1024;
    for long_names in [false, true] {
        let harness = Harness::new()?;
        let oracle = harness.directory.join("large-oracle");
        copy_oracle(
            &harness.root.join("docs/proposed/opentofu-goldens"),
            &oracle,
        )?;
        let extra = oracle.join("extra");
        fs::create_dir(&extra)?;
        let (baseline_count, baseline_bytes) = tree_usage(&oracle)?;
        let mut last = None;
        if long_names {
            let mut remaining = MAX_PATH_BYTES - baseline_bytes;
            let mut index = 0;
            while remaining > 0 {
                let mut length = remaining.min(246);
                if remaining > length && remaining - length < 11 {
                    length -= 11 - (remaining - length);
                }
                let name = format!("{index:05}{}", "x".repeat(length - 11));
                let path = extra.join(name);
                fs::write(&path, [])?;
                last = Some(path);
                remaining -= length;
                index += 1;
            }
            assert_eq!(tree_usage(&oracle)?.1, MAX_PATH_BYTES);
        } else {
            for index in 0..MAX_ENTRIES - baseline_count {
                fs::write(extra.join(format!("{index:05}")), [])?;
            }
            assert_eq!(tree_usage(&oracle)?.0, MAX_ENTRIES);
        }
        let check = || {
            harness.run(
                "check",
                &harness.root,
                &harness.guard_source,
                &harness.archive_guard,
                &harness.guard_sha256,
                Some(&oracle),
            )
        };
        let accepted = check()?;
        assert_version_failure(&accepted);
        if let Some(path) = last {
            let name = path
                .file_name()
                .ok_or("missing filename")?
                .to_string_lossy();
            fs::rename(&path, extra.join(format!("{name}x")))?;
            assert_eq!(tree_usage(&oracle)?.1, MAX_PATH_BYTES + 1);
        } else {
            fs::write(extra.join("one-more"), [])?;
            assert_eq!(tree_usage(&oracle)?.0, MAX_ENTRIES + 1);
        }
        let rejected = check()?;
        assert_eq!(
            rejected.status.code(),
            Some(2),
            "{}",
            String::from_utf8_lossy(&rejected.stderr)
        );
        assert!(
            String::from_utf8_lossy(&rejected.stderr).contains("tree exceeds its structural limit")
        );
        assert!(
            !String::from_utf8_lossy(&rejected.stderr)
                .contains("candidate --version command failed")
        );
    }
    Ok(())
}

#[test]
fn rejects_excessive_fixture_nesting_during_copy() -> Result<(), Box<dyn Error>> {
    let harness = Harness::new()?;
    let candidate = diagnostics_candidate()?;
    let source = harness.directory.join("source");
    super::mutation::copy_mutation_repository(&harness.root, &source)?;
    super::executable_custody::add_fixture_origin(&source)?;
    let mut path = source.join("fixtures/nested/extra");
    fs::create_dir(&path)?;
    for _ in 0..127 {
        path.push("d");
        fs::create_dir(&path)?;
    }
    let accepted_oracle = harness.directory.join("accepted-oracle");
    let accepted = harness
        .command(
            "capture",
            &source,
            &candidate,
            &harness.archive_guard,
            &harness.guard_sha256,
            Some(&accepted_oracle),
        )
        .env("VELNOR_PLAN_DIAGNOSTICS_MODE", "fail57")
        .env("VELNOR_PLAN_DIAGNOSTICS_ROOT", &source)
        .output()?;
    assert_eq!(accepted.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&accepted.stderr).contains("plan failed for nested (Exited(57))")
    );
    assert!(!accepted_oracle.exists());
    path.push("d");
    fs::create_dir(&path)?;
    let oracle = harness.directory.join("new-oracle");
    let output = harness
        .command(
            "capture",
            &source,
            &candidate,
            &harness.archive_guard,
            &harness.guard_sha256,
            Some(&oracle),
        )
        .env("VELNOR_PLAN_DIAGNOSTICS_MODE", "fail57")
        .env("VELNOR_PLAN_DIAGNOSTICS_ROOT", &source)
        .output()?;
    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("tree exceeds its structural limit"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("plan failed for nested"));
    assert!(!oracle.exists());
    Ok(())
}

fn assert_version_failure(output: &std::process::Output) {
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("candidate --version command failed"));
}

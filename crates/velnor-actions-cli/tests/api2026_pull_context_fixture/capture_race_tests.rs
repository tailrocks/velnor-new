// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, path::Path, process::Command};

fn git(root: &Path, args: &[&str]) -> Result<(), Box<dyn Error>> {
    let status = Command::new("/usr/bin/git")
        .current_dir(root)
        .args(args)
        .status()?;
    if !status.success() {
        return Err("fixture Git failed".into());
    }
    Ok(())
}

fn capture(root: &Path, output: &Path, manifest: &Path) -> Result<(), Box<dyn Error>> {
    std::fs::create_dir(output)?;
    let status = Command::new("/usr/bin/python3")
        .args(["-I", "-S", "-B"])
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/api2026_pull_context_fixture/raw_capture.py"
        ))
        .arg(root)
        .arg(output)
        .arg(manifest)
        .status()?;
    if !status.success() {
        return Err("fixture capture failed".into());
    }
    crate::reviewed_input_helper::capture_verify::verify_capture(
        output,
        &std::fs::read_to_string(manifest)?,
    )?;
    Ok(())
}

#[test]
fn captured_planner_isolated_from_live_edits_and_hydrates_symlink_target()
-> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("repo");
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or("root")?;
    let status = Command::new("/usr/bin/git")
        .env("GIT_LFS_SKIP_SMUDGE", "1")
        .args(["clone", "--no-local", "--quiet"])
        .arg(root)
        .arg(&repo)
        .status()?;
    assert!(status.success());
    git(
        &repo,
        &[
            "remote",
            "set-url",
            "origin",
            "https://github.com/tailrocks/velnor-new.git",
        ],
    )?;
    let config_path = repo.join(".velnor/config.toml");
    let original = std::fs::read_to_string(&config_path)?;
    let config = original.replace(
        "[stacks.rust]\n",
        "[stacks.rust]\nnon_inputs = [\"docs/implemented/**\"]\n",
    );
    let config = config.replace("[[stacks.rust.configurations]]", concat!(
        "[[stacks.rust.inputs]]\n",
        "globs = [\"*\", \".cargo/**\", \".config/**\", \".github/**\", \".mise/**\", \".velnor/**\", \"LICENSES/**\", \"crates/**\", \"docs/*\", \"docs/content/**\", \"docs/deferred/**\", \"docs/proposed/**\", \"docs/reviews/**\", \"fixtures/**\", \"images/**\", \"qualification/**\", \"scripts/**\"]\n",
        "packages = [\"velnor-actions-contract\", \"velnor-actions-freshness\", \"velnor-actions-mise\", \"velnor-actions-orchestrator\", \"velnor-actions-rust\", \"velnor-actions-workflow-renderer\", \"velnor-archive-guard\", \"velnor-runner-attestation\"]\ncomplete = true\n\n[[stacks.rust.configurations]]"
    ));
    assert_ne!(original, config);
    std::fs::write(&config_path, &config)?;
    let doc = "docs/implemented/fork-pr-trust-model.md";
    git(&repo, &["update-index", "--skip-worktree", doc])?;
    std::fs::remove_file(repo.join(doc))?;
    let first = temp.path().join("first");
    capture(&repo, &first, &temp.path().join("first.json"))?;
    std::fs::write(&config_path, &original)?;
    std::fs::write(
        repo.join("crates/velnor-actions-cli/src/main.rs"),
        b"live source changed",
    )?;
    let patterns = crate::reviewed_input_helper::reviewed_non_inputs_from(&first)?;
    assert_eq!(patterns, "[\"docs/implemented/**\"]");
    assert_eq!(
        std::fs::read_to_string(first.join(".velnor/config.toml"))?,
        config
    );
    assert_ne!(
        std::fs::read(first.join("crates/velnor-actions-cli/src/main.rs"))?,
        b"live source changed"
    );
    let projection = temp.path().join("projection");
    std::fs::create_dir(&projection)?;
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/api2026_pull_context_fixture");
    let status = Command::new("/usr/bin/python3")
        .args(["-I", "-S", "-B", "-c"])
        .arg("import json,sys; sys.path.insert(0,sys.argv[1]); import raw_capture; raw_capture.project(sys.argv[2],sys.argv[3],json.load(open(sys.argv[4])),json.loads(sys.argv[5]))")
        .arg(fixture)
        .arg(&first)
        .arg(&projection)
        .arg(temp.path().join("first.json"))
        .arg(&patterns)
        .status()?;
    assert!(status.success());
    assert!(!projection.join(doc).exists());
    assert_eq!(
        std::fs::read_to_string(projection.join(".velnor/config.toml"))?,
        config
    );
    std::fs::write(&config_path, &config)?;
    symlink(doc, repo.join("excluded-link-audit"))?;
    git(&repo, &["add", "excluded-link-audit"])?;
    let second = temp.path().join("second");
    capture(&repo, &second, &temp.path().join("second.json"))?;
    assert!(
        second.join(doc).exists(),
        "sparse blob must be hydrated before validation"
    );
    let error = crate::reviewed_input_helper::reviewed_non_inputs_from(&second)
        .expect_err("symlink target overlap must fail");
    assert!(
        error
            .to_string()
            .contains("non_input_overlaps_claim:symlink"),
        "{error}"
    );
    Ok(())
}

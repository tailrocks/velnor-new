// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs, path::Path, process::Command};

fn git(root: &Path, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let output = Command::new("git")
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "Fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err(format!("git failed: {}", String::from_utf8_lossy(&output.stderr)).into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

#[test]
fn qualification_accepts_advanced_base_and_rejects_unrelated_event_shas()
-> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let source = root.join("source");
    let origin = root.join("origin.git");
    let checkout = root.join("checkout");
    let runner = root.join("runner");
    fs::create_dir(&source)?;
    fs::create_dir(&checkout)?;
    fs::create_dir_all(runner.join("source-qualification-artifacts"))?;
    git(&source, &["init", "--quiet", "-b", "main"])?;
    fs::write(source.join("initial"), b"initial")?;
    git(&source, &["add", "."])?;
    git(&source, &["commit", "--quiet", "-m", "initial"])?;
    let stale_base = git(&source, &["rev-parse", "HEAD"])?;
    git(&source, &["checkout", "--quiet", "-b", "topic"])?;
    fs::write(source.join("topic"), b"topic")?;
    git(&source, &["add", "."])?;
    git(&source, &["commit", "--quiet", "-m", "topic"])?;
    let head = git(&source, &["rev-parse", "HEAD"])?;
    git(&source, &["checkout", "--quiet", "main"])?;
    fs::write(source.join("base"), b"base")?;
    git(&source, &["add", "."])?;
    git(&source, &["commit", "--quiet", "-m", "base"])?;
    let base = git(&source, &["rev-parse", "HEAD"])?;
    git(
        &source,
        &["merge", "--quiet", "--no-ff", "topic", "-m", "merge"],
    )?;
    let merge = git(&source, &["rev-parse", "HEAD"])?;
    git(
        &root,
        &[
            "init",
            "--quiet",
            "--bare",
            origin.to_str().ok_or("origin")?,
        ],
    )?;
    git(
        &source,
        &["remote", "add", "origin", origin.to_str().ok_or("origin")?],
    )?;
    git(
        &source,
        &["push", "--quiet", "origin", "HEAD:refs/heads/main"],
    )?;
    git(&checkout, &["init", "--quiet"])?;
    let url = format!("file://{}", origin.display());
    git(&checkout, &["remote", "add", "origin", &url])?;
    git(
        &checkout,
        &["fetch", "--quiet", "--depth=1", "origin", &merge],
    )?;
    git(
        &checkout,
        &["checkout", "--quiet", "--detach", "FETCH_HEAD"],
    )?;
    assert!(
        git(
            &checkout,
            &["cat-file", "-e", &format!("{base}^{{commit}}")]
        )
        .is_err()
    );
    check_results(&checkout, &runner, &merge, &stale_base, &base, &head)?;
    fetches_base_history_without_trees(&checkout, &runner, &origin, &merge, &stale_base, &head)?;
    rejects_first_parent_outside_base_branch(&root, &source, &origin, &stale_base, &base, &head)
}

fn rejects_first_parent_outside_base_branch(
    root: &Path,
    source: &Path,
    origin: &Path,
    stale_base: &str,
    base: &str,
    head: &str,
) -> Result<(), Box<dyn Error>> {
    git(source, &["checkout", "--quiet", "-b", "side", base])?;
    fs::write(source.join("side"), b"side")?;
    git(source, &["add", "."])?;
    git(source, &["commit", "--quiet", "-m", "side"])?;
    git(
        source,
        &["merge", "--quiet", "--no-ff", "topic", "-m", "side merge"],
    )?;
    let side_merge = git(source, &["rev-parse", "HEAD"])?;
    git(
        source,
        &["push", "--quiet", "origin", "HEAD:refs/heads/side"],
    )?;
    let checkout = root.join("checkout-side");
    let runner = root.join("runner-side");
    fs::create_dir(&checkout)?;
    fs::create_dir_all(runner.join("source-qualification-artifacts"))?;
    git(&checkout, &["init", "--quiet"])?;
    let url = format!("file://{}", origin.display());
    git(&checkout, &["remote", "add", "origin", &url])?;
    git(
        &checkout,
        &["fetch", "--quiet", "--depth=1", "origin", &side_merge],
    )?;
    git(
        &checkout,
        &["checkout", "--quiet", "--detach", "FETCH_HEAD"],
    )?;
    let off_branch = run_script(&checkout, &runner, &side_merge, stale_base, head)?;
    assert!(!off_branch.status.success());
    assert!(
        String::from_utf8_lossy(&off_branch.stderr).contains("not on the base branch"),
        "{}",
        String::from_utf8_lossy(&off_branch.stderr)
    );
    Ok(())
}

fn fetches_base_history_without_trees(
    checkout: &Path,
    runner: &Path,
    origin: &Path,
    merge: &str,
    stale_base: &str,
    head: &str,
) -> Result<(), Box<dyn Error>> {
    fs::remove_file(checkout.join("untracked"))?;
    git(origin, &["config", "uploadpack.allowFilter", "true"])?;
    let shallow_before = fs::read(checkout.join(".git/shallow")).ok();
    let traced = run_script_with(
        checkout,
        runner,
        merge,
        stale_base,
        head,
        &[("GIT_TRACE_PACKET", "1")],
    )?;
    assert!(
        traced.status.success(),
        "{}",
        String::from_utf8_lossy(&traced.stderr)
    );
    assert!(
        String::from_utf8_lossy(&traced.stderr).contains("filter tree:0"),
        "the base history fetch must request a treeless filter"
    );
    assert_eq!(
        shallow_before,
        fs::read(checkout.join(".git/shallow")).ok(),
        "the checkout's own history must not be rewritten"
    );
    assert!(!runner.join("base-branch-history.git").exists());
    let evidence =
        fs::read_to_string(runner.join("source-qualification-artifacts/source-identity.txt"))?;
    assert!(evidence.contains(&format!("pr_base_sha={stale_base}\n")));
    git(origin, &["config", "uploadpack.allowFilter", "false"])?;
    Ok(())
}

fn run_script(
    checkout: &Path,
    runner: &Path,
    merge: &str,
    event_base: &str,
    event_head: &str,
) -> Result<std::process::Output, Box<dyn Error>> {
    run_script_with(checkout, runner, merge, event_base, event_head, &[])
}

fn run_script_with(
    checkout: &Path,
    runner: &Path,
    merge: &str,
    event_base: &str,
    event_head: &str,
    extra_env: &[(&str, &str)],
) -> Result<std::process::Output, Box<dyn Error>> {
    let script = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../scripts/generator-release/source-qualification.sh"
    );
    Ok(Command::new("bash")
        .arg(script)
        .arg("verify-exact-clean-source-checkout")
        .current_dir(checkout)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("RUNNER_TEMP", runner)
        .env("GITHUB_SHA", merge)
        .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
        .env("GITHUB_EVENT_NAME", "pull_request")
        .env("GITHUB_REF", "refs/pull/1/merge")
        .env(
            "GITHUB_WORKFLOW_REF",
            "tailrocks/velnor-new/.github/workflows/source-qualification.yml@refs/pull/1/merge",
        )
        .env("GITHUB_WORKFLOW_SHA", merge)
        .env("RUNNER_OS", "Linux")
        .env("RUNNER_ARCH", "X64")
        .env("PR_BASE_SHA", event_base)
        .env("PR_BASE_REF", "main")
        .env("PR_HEAD_SHA", event_head)
        .envs(extra_env.iter().copied())
        .output()?)
}

fn check_results(
    checkout: &Path,
    runner: &Path,
    merge: &str,
    stale_base: &str,
    base: &str,
    head: &str,
) -> Result<(), Box<dyn Error>> {
    let run = |event_base: &str, event_head: &str| {
        run_script(checkout, runner, merge, event_base, event_head)
    };
    let advanced = run(stale_base, head)?;
    assert!(
        advanced.status.success(),
        "{}",
        String::from_utf8_lossy(&advanced.stderr)
    );
    let advanced_evidence =
        fs::read_to_string(runner.join("source-qualification-artifacts/source-identity.txt"))?;
    assert!(advanced_evidence.contains(&format!("pr_base_sha={stale_base}\n")));
    assert!(advanced_evidence.contains(&format!("merge_parent_1={base}\n")));
    assert!(advanced_evidence.contains(&format!("merge_parent_2={head}\n")));
    let not_an_ancestor = run(head, head)?;
    assert!(!not_an_ancestor.status.success());
    assert!(
        String::from_utf8_lossy(&not_an_ancestor.stderr)
            .contains("pull request merge parents do not match")
    );
    let absent = run(&"f".repeat(40), head)?;
    assert!(!absent.status.success());
    assert!(
        String::from_utf8_lossy(&absent.stderr).contains("pull request merge parents do not match")
    );
    let matching = run(base, head)?;
    assert!(
        matching.status.success(),
        "{}",
        String::from_utf8_lossy(&matching.stderr)
    );
    let evidence =
        fs::read_to_string(runner.join("source-qualification-artifacts/source-identity.txt"))?;
    assert!(evidence.contains(&format!("pr_base_sha={base}\n")));
    assert!(evidence.contains(&format!("pr_head_sha={head}\n")));
    assert!(evidence.contains(&format!("merge_parent_1={base}\n")));
    assert!(evidence.contains(&format!("merge_parent_2={head}\n")));
    let wrong_head = run(base, stale_base)?;
    assert!(!wrong_head.status.success());
    assert!(
        String::from_utf8_lossy(&wrong_head.stderr)
            .contains("pull request merge parents do not match")
    );
    let malformed = run("--upload-pack=ignored", head)?;
    assert!(!malformed.status.success());
    fs::write(checkout.join("untracked"), b"dirty")?;
    let dirty = run(base, head)?;
    assert!(!dirty.status.success());
    Ok(())
}

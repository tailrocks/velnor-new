//! Finding 1, executed: prepare's `.github/` reaches the branch only as the
//! verify job's own regeneration with a Velnor release it verified.

use std::fs;

use super::exec_support::{RELEASE_COMMIT, Sandbox, TestResult, generated_ci, text};

/// The patch never reaches a token or the branch, with `code`.
fn assert_refused(sandbox: &Sandbox, code: &str) -> TestResult {
    let run = sandbox.pipeline()?;
    let log = run.log();
    assert!(run.publish.is_none(), "the patch reached publish:\n{log}");
    assert!(
        log.contains(&format!("::error::{code}: ")),
        "{code}:\n{log}"
    );
    assert!(!sandbox.token_minted(), "a token was minted:\n{log}");
    assert_eq!(sandbox.pushed_branch()?, None, "pushed:\n{log}");
    Ok(())
}

#[test]
fn injected_workflow_from_prepare_never_reaches_the_branch() -> TestResult {
    // The review's reproduction: a poisoned dependency in prepare writes
    // `.github/workflows/evil.yml` next to a legitimate Velnor bump. The old
    // publish job pushed it with a `workflows: write` token.
    let sandbox = Sandbox::new("evil-workflow")?;
    sandbox.release("0.2.0")?;
    sandbox.bump_velnor("0.2.0")?;
    sandbox.write("Cargo.lock", "version = 4\n# bumped\n")?;
    sandbox.write(
        ".github/workflows/evil.yml",
        "on: push\njobs:\n  x:\n    runs-on: ubuntu-latest\n    steps:\n      - run: env\n",
    )?;
    sandbox.capture_patch(&[])?;
    assert_refused(&sandbox, "upgrade_patch_github_mismatch")
}

#[test]
fn rewritten_generated_workflow_is_refused() -> TestResult {
    let sandbox = Sandbox::new("rewrite-ci")?;
    sandbox.write(".github/workflows/ci.yml", "on: push\nname: CI\n")?;
    sandbox.capture_patch(&[])?;
    assert_refused(&sandbox, "upgrade_patch_github_mismatch")
}

#[test]
fn verified_velnor_bump_is_published_with_the_workflows_permission() -> TestResult {
    let sandbox = Sandbox::new("bump")?;
    sandbox.release("0.2.0")?;
    sandbox.bump_velnor("0.2.0")?;
    sandbox.write("Cargo.lock", "version = 4\n# bumped\n")?;
    sandbox.capture_patch(&[])?;
    let run = sandbox.pipeline()?;
    let publish = run.publish.as_ref().ok_or_else(|| run.log())?;
    assert!(publish.status.success(), "{}", run.log());
    assert_eq!(
        run.outputs.get("workflows").map(String::as_str),
        Some("write")
    );
    assert_eq!(
        sandbox.pushed_file(".github/workflows/ci.yml")?,
        generated_ci("0.2.0")
    );
    let tree = sandbox.pushed_file("Cargo.lock")?;
    assert_eq!(tree, "version = 4\n# bumped\n");
    // The verify job regenerated with the binary it verified itself.
    let calls = sandbox.calls()?;
    assert!(
        calls.contains(&format!(
            "gh attestation verify {}",
            sandbox
                .work
                .join("velnor/velnor-actions-0.2.0-x86_64-unknown-linux-gnu")
                .display()
        )),
        "{calls}"
    );
    assert!(
        calls.contains(&format!("--source-digest {RELEASE_COMMIT}")),
        "{calls}"
    );
    Ok(())
}

#[test]
fn dependency_only_upgrade_requests_no_workflows_permission() -> TestResult {
    let sandbox = Sandbox::new("deps-only")?;
    sandbox.write("Cargo.lock", "version = 4\n# bumped\n")?;
    sandbox.capture_patch(&[])?;
    let run = sandbox.pipeline()?;
    let publish = run.publish.as_ref().ok_or_else(|| run.log())?;
    assert!(publish.status.success(), "{}", run.log());
    assert_eq!(run.outputs.get("workflows").map(String::as_str), Some(""));
    Ok(())
}

#[test]
fn patch_selecting_an_unverified_velnor_is_refused() -> TestResult {
    // Prepare claims 0.2.0 while the newest verified stable release is 0.1.0.
    let sandbox = Sandbox::new("unverified-manifest")?;
    sandbox.release("0.2.0")?;
    sandbox.bump_velnor("0.2.0")?;
    sandbox.capture_patch(&[])?;
    fs::write(
        sandbox.root.join("releases.json"),
        "[{\"tag_name\":\"v0.1.0\",\"draft\":false,\"prerelease\":false}]\n",
    )?;
    assert_refused(&sandbox, "upgrade_velnor_manifest_unverified")
}

#[test]
fn release_without_valid_provenance_is_refused() -> TestResult {
    let sandbox = Sandbox::new("provenance")?;
    sandbox.release("0.2.0")?;
    sandbox.bump_velnor("0.2.0")?;
    sandbox.capture_patch(&[])?;
    let binary = sandbox
        .release_dir("0.2.0")
        .join("velnor-actions-0.2.0-x86_64-unknown-linux-gnu.intoto.jsonl");
    fs::write(binary, "attested 0000 elsewhere")?;
    assert_refused(&sandbox, "upgrade_velnor_provenance_unverified")
}

#[test]
fn release_binary_that_differs_from_its_manifest_digest_is_refused() -> TestResult {
    let sandbox = Sandbox::new("digest")?;
    sandbox.release("0.2.0")?;
    sandbox.bump_velnor("0.2.0")?;
    sandbox.capture_patch(&[])?;
    let name = "velnor-actions-0.2.0-x86_64-unknown-linux-gnu";
    fs::write(
        sandbox.release_dir("0.2.0").join(name),
        "#!/bin/sh\nmkdir -p .github/workflows\necho 'on: push' > .github/workflows/evil.yml\n",
    )?;
    // Even a re-attested substitute fails the manifest digest.
    sandbox.attest("0.2.0", name)?;
    assert_refused(&sandbox, "upgrade_velnor_digest_mismatch")
}

#[test]
fn publish_refuses_any_patch_other_than_the_verified_one() -> TestResult {
    let sandbox = Sandbox::new("swap")?;
    sandbox.write("Cargo.lock", "version = 4\n# bumped\n")?;
    sandbox.capture_patch(&[])?;
    sandbox.reset()?;
    assert!(sandbox.verify_release()?.status.success());
    let verify = sandbox.run(&super::verify_script(), &[])?;
    assert!(verify.status.success(), "{}", text(&verify));
    let outputs = sandbox.outputs();
    // Swap the artifact after verification for one that adds a workflow.
    sandbox.reset()?;
    sandbox.write(".github/workflows/evil.yml", "on: push\n")?;
    sandbox.capture_patch(&[])?;
    sandbox.reset()?;
    let publish = sandbox.publish(&outputs)?;
    let log = text(&publish);
    assert!(!publish.status.success(), "{log}");
    assert!(log.contains("::error::upgrade_patch_unverified: "), "{log}");
    assert!(!sandbox.token_minted(), "{log}");
    assert_eq!(sandbox.pushed_branch()?, None);
    Ok(())
}

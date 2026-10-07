//! P10 NUL-path cases: exact path bytes (Unicode, spaces, newlines,
//! deletes, renames, staged/unstaged union, non-UTF-8) through the typed
//! plan boundary.
//!
//! APFS rejects non-UTF-8 names at creation, so the committed case builds
//! its commit through git plumbing ([`commit_with_raw_name`]) and runs
//! everywhere; the untracked case cannot exist on such filesystems (an
//! untracked path must be created to be listed) and reports a loud
//! `paths: ... status=SKIP` diagnostic instead of asserting.

use std::fs;
use std::path::Path;
#[cfg(unix)]
use std::process::Command as StdCommand;

use velnor_actions_contract_workflow::Plan;
use velnor_actions_orchestrator::plan_internal;
use velnor_actions_orchestrator_core::resolve_root;

use crate::impl_common::{TestResult, git};
use crate::impl_select::{commit, make_ws_repo, plan_pr, reasons_for};

/// Plan the working tree against `HEAD` as a local run.
fn plan_local(root: &Path, head: &str) -> Result<(Plan, Vec<String>), Box<dyn std::error::Error>> {
    let request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "base": None::<String>,
        "head": head,
        "event": "local",
        "root": root.display().to_string(),
    });
    let response = plan_internal(&request.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&response)?;
    let plan: Plan = serde_json::from_value(value["plan"].clone())?;
    let warnings: Vec<String> = serde_json::from_value(value["plan"]["warnings"].clone())?;
    Ok((plan, warnings))
}

/// Assert one member changed and the other stayed forced-uncached.
fn assert_narrow(plan: &Plan, changed: &str, other: &str) {
    let hit = reasons_for(plan, changed);
    assert!(
        hit.iter().all(|r| *r == "affected_by_change"),
        "{changed}: {hit:?}"
    );
    let miss = reasons_for(plan, other);
    assert!(
        miss.iter().all(|r| *r == "forced_uncached"),
        "{other}: {miss:?}"
    );
}

#[test]
fn unicode_filename_selects_owner_only() -> TestResult {
    let repo = make_ws_repo(false)?;
    let root = repo.path();
    fs::write(root.join("alpha/src/héllo.rs"), "pub fn f() {}\n")?;
    let base = commit(root, "one")?;
    fs::write(
        root.join("alpha/src/héllo.rs"),
        "pub fn f() {}\npub fn g() {}\n",
    )?;
    let head = commit(root, "two")?;
    let (plan, warnings) = plan_pr(root, Some(&base), &head)?;
    assert_narrow(&plan, "alpha", "beta");
    assert!(warnings.is_empty(), "no warnings: {warnings:?}");
    Ok(())
}

#[test]
fn trailing_space_filename_selects_owner_only() -> TestResult {
    let repo = make_ws_repo(false)?;
    let root = repo.path();
    let base = commit(root, "one")?;
    fs::write(root.join("alpha/src/trailing.rs "), "pub fn f() {}\n")?;
    let head = commit(root, "two")?;
    let (plan, warnings) = plan_pr(root, Some(&base), &head)?;
    assert_narrow(&plan, "alpha", "beta");
    assert!(warnings.is_empty(), "no warnings: {warnings:?}");
    Ok(())
}

#[test]
fn whitespace_only_filename_not_dropped() -> TestResult {
    let repo = make_ws_repo(false)?;
    let root = repo.path();
    let base = commit(root, "one")?;
    fs::write(root.join("   "), "notes\n")?;
    let head = commit(root, "two")?;
    let (plan, warnings) = plan_pr(root, Some(&base), &head)?;
    assert!(
        plan.task_ids.iter().any(|id| id.contains("alpha"))
            && plan.task_ids.iter().any(|id| id.contains("beta")),
        "unowned change broadens: {:?}",
        plan.task_ids
    );
    assert!(
        warnings.iter().any(|w| w.contains("unclassified_files")),
        "tag: {warnings:?}"
    );
    Ok(())
}

#[test]
fn newline_filename_selects_owner_only() -> TestResult {
    let repo = make_ws_repo(false)?;
    let root = repo.path();
    let base = commit(root, "one")?;
    fs::write(root.join("alpha/src/with\nnewline.rs"), "pub fn f() {}\n")?;
    let head = commit(root, "two")?;
    let (plan, warnings) = plan_pr(root, Some(&base), &head)?;
    assert_narrow(&plan, "alpha", "beta");
    assert!(warnings.is_empty(), "no warnings: {warnings:?}");
    Ok(())
}

#[test]
fn deleted_path_selects_owner_only() -> TestResult {
    let repo = make_ws_repo(false)?;
    let root = repo.path();
    fs::write(root.join("beta/src/extra.rs"), "pub fn extra() {}\n")?;
    let base = commit(root, "one")?;
    fs::remove_file(root.join("beta/src/extra.rs"))?;
    let head = commit(root, "two")?;
    let (plan, warnings) = plan_pr(root, Some(&base), &head)?;
    assert_narrow(&plan, "beta", "alpha");
    assert!(warnings.is_empty(), "no warnings: {warnings:?}");
    Ok(())
}

#[test]
fn renamed_path_selects_owner_only() -> TestResult {
    let repo = make_ws_repo(false)?;
    let root = repo.path();
    fs::write(root.join("beta/src/extra.rs"), "pub fn extra() {}\n")?;
    let base = commit(root, "one")?;
    git(&["mv", "beta/src/extra.rs", "beta/src/renamed.rs"], root)?;
    let head = commit(root, "two")?;
    let (plan, warnings) = plan_pr(root, Some(&base), &head)?;
    assert_narrow(&plan, "beta", "alpha");
    assert!(warnings.is_empty(), "no warnings: {warnings:?}");
    Ok(())
}

#[test]
fn local_staged_and_unstaged_union_selects() -> TestResult {
    let repo = make_ws_repo(false)?;
    let root = repo.path();
    let head = commit(root, "one")?;
    fs::write(
        root.join("alpha/src/lib.rs"),
        "pub fn f() {}\npub fn a() {}\n",
    )?;
    fs::write(
        root.join("beta/src/lib.rs"),
        "pub fn f() {}\npub fn b() {}\n",
    )?;
    git(&["add", "beta/src/lib.rs"], root)?;
    let (plan, warnings) = plan_local(root, &head)?;
    for member in ["alpha", "beta"] {
        let reasons = reasons_for(&plan, member);
        assert!(
            reasons.iter().all(|r| *r == "affected_by_change"),
            "{member}: {reasons:?}"
        );
    }
    assert!(warnings.is_empty(), "no warnings: {warnings:?}");
    Ok(())
}

#[test]
fn local_unicode_change_selects_owner_only() -> TestResult {
    let repo = make_ws_repo(false)?;
    let root = repo.path();
    fs::write(root.join("alpha/src/héllo.rs"), "pub fn f() {}\n")?;
    let head = commit(root, "one")?;
    fs::write(
        root.join("alpha/src/héllo.rs"),
        "pub fn f() {}\npub fn g() {}\n",
    )?;
    let (plan, warnings) = plan_local(root, &head)?;
    assert_narrow(&plan, "alpha", "beta");
    assert!(warnings.is_empty(), "no warnings: {warnings:?}");
    Ok(())
}

#[test]
fn nested_subdir_resolves_repo_root() -> TestResult {
    let repo = make_ws_repo(false)?;
    let root = repo.path();
    commit(root, "one")?;
    let subdir = root.join("alpha/src");
    assert_eq!(resolve_root(&subdir)?, root.canonicalize()?);
    Ok(())
}

#[test]
fn added_member_manifest_narrows_without_broaden() -> TestResult {
    let repo = make_ws_repo(false)?;
    let root = repo.path();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"alpha\", \"beta\", \"gamma\"]\n",
    )?;
    let base = commit(root, "one")?;
    fs::create_dir_all(root.join("gamma/src"))?;
    fs::write(
        root.join("gamma/Cargo.toml"),
        "[package]\nname = \"gamma\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::write(root.join("gamma/src/lib.rs"), "pub fn f() {}\n")?;
    let head = commit(root, "two")?;
    let (plan, warnings) = plan_pr(root, Some(&base), &head)?;
    let hit = reasons_for(&plan, "gamma");
    assert!(
        hit.iter().all(|r| *r == "affected_by_change"),
        "gamma: {hit:?}"
    );
    for other in ["alpha", "beta"] {
        let miss = reasons_for(&plan, other);
        assert!(
            miss.iter().all(|r| *r == "forced_uncached"),
            "{other}: {miss:?}"
        );
    }
    assert!(warnings.is_empty(), "no warnings: {warnings:?}");
    Ok(())
}

/// An added manifest with a quotepath-triggering name fails closed explicitly.
///
/// Non-ASCII members are rejected by obligation-time manifest-key validation
/// before selection runs, so this pins the explicit error (not silent
/// broaden) for unsupported member names. Byte-exactness of `added_files`
/// itself is enforced by construction (trusted `-z` + shared splitter) and
/// proven at the splitter unit level, since only validated manifests are
/// ever matched against the added set.
#[test]
fn added_quotepath_manifest_fails_closed() -> TestResult {
    let repo = make_ws_repo(false)?;
    let root = repo.path();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"alpha\", \"beta\", \"gamm\u{e1}\"]\n",
    )?;
    let base = commit(root, "one")?;
    fs::create_dir_all(root.join("gamm\u{e1}/src"))?;
    fs::write(
        root.join("gamm\u{e1}/Cargo.toml"),
        "[package]\nname = \"gamma\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::write(root.join("gamm\u{e1}/src/lib.rs"), "pub fn f() {}\n")?;
    let head = commit(root, "two")?;
    let error = plan_pr(root, Some(&base), &head).expect_err("must fail closed");
    assert!(
        error.to_string().contains("bad_component"),
        "names the problem: {error}"
    );
    Ok(())
}

#[test]
#[cfg(unix)]
fn untracked_non_utf8_broadens_with_explicit_tag() -> TestResult {
    use std::os::unix::ffi::OsStrExt;
    let repo = make_ws_repo(false)?;
    let root = repo.path();
    let base = commit(root, "one")?;
    let raw = b"\xffuntracked.rs";
    let path = root.join(std::ffi::OsStr::from_bytes(raw));
    if fs::write(&path, "pub fn f() {}\n").is_err() {
        eprintln!(
            "paths: case=untracked_non_utf8 status=SKIP note=filesystem-rejects-non-utf8-names"
        );
        return Ok(());
    }
    let (plan, warnings) = plan_pr(root, Some(&base), &base)?;
    assert!(
        plan.task_ids.iter().any(|id| id.contains("alpha"))
            && plan.task_ids.iter().any(|id| id.contains("beta")),
        "broadens: {:?}",
        plan.task_ids
    );
    assert!(
        warnings
            .iter()
            .any(|w| w == "comparison_unavailable:non_utf8_path:all_changed"),
        "explicit tag: {warnings:?}"
    );
    Ok(())
}

/// Commit a tree entry whose name bytes no filesystem may carry.
///
/// `git update-index --cacheinfo` inserts the blob under raw name bytes
/// without touching the worktree, so the committed non-UTF-8 path runs
/// hermetically — including on APFS, which rejects such names at
/// creation. HEAD advances to the new commit (the plan boundary requires
/// the checkout to match `head`) and the index is reset to match.
#[cfg(unix)]
fn commit_with_raw_name(
    root: &Path,
    base: &str,
    raw: &[u8],
) -> Result<String, Box<dyn std::error::Error>> {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    fn git_output(args: &[OsString], cwd: &Path) -> Result<String, Box<dyn std::error::Error>> {
        let output = StdCommand::new("git")
            .args(args)
            .current_dir(cwd)
            .output()?;
        assert!(
            output.status.success(),
            "git {args:?} failed in {}",
            cwd.display()
        );
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    let hash_child = StdCommand::new("git")
        .args(["hash-object", "-w", "-t", "blob", "--stdin"])
        .current_dir(root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .spawn()?;
    let hash_out = hash_child.wait_with_output()?;
    assert!(hash_out.status.success(), "hash-object failed");
    let blob = String::from_utf8_lossy(&hash_out.stdout).trim().to_owned();
    assert_eq!(blob.len(), 40, "blob sha: {blob}");

    let mode = OsString::from(format!("100644,{blob},"));
    let mut spec = mode.into_vec();
    spec.extend_from_slice(raw);
    let cacheinfo = OsString::from_vec(spec);
    git_output(
        &[
            OsString::from("update-index"),
            OsString::from("--add"),
            OsString::from("--cacheinfo"),
            cacheinfo,
        ],
        root,
    )?;
    let tree = git_output(&[OsString::from("write-tree")], root)?;
    let head = git_output(
        &[
            OsString::from("commit-tree"),
            OsString::from(tree),
            OsString::from("-p"),
            OsString::from(base),
            OsString::from("-m"),
            OsString::from("two"),
        ],
        root,
    )?;
    git_output(
        &[
            OsString::from("update-ref"),
            OsString::from("HEAD"),
            OsString::from(&head),
        ],
        root,
    )?;
    git_output(&[OsString::from("reset"), OsString::from("-q")], root)?;
    Ok(head)
}

#[test]
#[cfg(unix)]
fn non_utf8_path_broadens_explicitly() -> TestResult {
    let repo = make_ws_repo(false)?;
    let root = repo.path();
    let base = commit(root, "one")?;
    let head = commit_with_raw_name(root, &base, b"alpha/src/\xffinvalid.rs")?;
    let (plan, warnings) = plan_pr(root, Some(&base), &head)?;
    assert!(
        plan.task_ids.iter().any(|id| id.contains("alpha"))
            && plan.task_ids.iter().any(|id| id.contains("beta")),
        "broadens: {:?}",
        plan.task_ids
    );
    assert!(
        warnings.iter().any(|w| w.contains("non_utf8_path")),
        "explicit tag: {warnings:?}"
    );
    Ok(())
}

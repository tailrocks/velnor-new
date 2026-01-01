//! Public discovery behavior for generated cache content.

use std::fs;
use std::path::Path;

use crate::support::{TestResult, config_with_branch, git, git_line, make_repo};
use velnor_actions_orchestrator::prepare;

/// Write a minimal nested Rust crate for discovery assertions.
fn write_crate(root: &Path, relative: &str) -> TestResult {
    let directory = root.join(relative);
    let name = relative.replace('.', "x").replace('/', "-");
    fs::create_dir_all(directory.join("src"))?;
    fs::write(
        directory.join("Cargo.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
    )?;
    fs::write(directory.join("src/lib.rs"), "pub fn fixture() {}\n")?;
    Ok(())
}

/// Selected Rust project roots exposed by the public preparation API.
fn selected_roots(root: &Path) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let preparation = prepare(root)?;
    Ok(preparation
        .discovery
        .statuses
        .iter()
        .filter_map(|status| match status {
            velnor_actions_contract_planning::DetectionStatus::Selected(project) => {
                Some(project.project_root.clone())
            }
            velnor_actions_contract_planning::DetectionStatus::Ignored { .. } => None,
        })
        .collect())
}

/// A tracked cache payload fails through the public planning boundary.
#[test]
fn tracked_cache_payload_fails_public_discovery() -> TestResult {
    let repository = make_repo(config_with_branch())?;
    let root = repository.path();
    write_crate(root, ".velnor/cache/cargo/registry/cache/payload")?;
    git(&["add", ".velnor/cache"], root)?;

    let error = prepare(root).expect_err("tracked cache content must fail closed");
    assert!(
        error
            .to_string()
            .contains("tracked_reserved_cache_path:.velnor/cache/"),
        "unexpected error: {error}"
    );
    Ok(())
}

/// A tracked gitlink at the cache root fails through public discovery.
#[test]
fn tracked_cache_gitlink_fails_public_discovery() -> TestResult {
    let repository = make_repo(config_with_branch())?;
    let root = repository.path();
    let cache = root.join(".velnor/cache");
    fs::create_dir_all(&cache)?;
    git(&["init", "-q"], &cache)?;
    git(&["config", "user.name", "Velnor Test"], &cache)?;
    git(
        &["config", "user.email", "velnor-test@example.invalid"],
        &cache,
    )?;
    git(&["commit", "--allow-empty", "-m", "cache"], &cache)?;
    git(&["add", "-f", ".velnor/cache"], root)?;
    let staged = git_line(&["ls-files", "-s"], root)?;
    assert!(
        staged.contains("160000 ") && staged.contains("\t.velnor/cache"),
        "fixture must stage a gitlink: {staged}"
    );

    let error = prepare(root).expect_err("tracked cache gitlink must fail closed");
    assert!(
        error
            .to_string()
            .contains("tracked_reserved_cache_path:.velnor/cache"),
        "unexpected error: {error}"
    );
    Ok(())
}

/// Untracked cache is omitted, while exact-name siblings remain selected.
#[test]
fn untracked_cache_is_excluded_and_siblings_remain_discoverable() -> TestResult {
    let repository = make_repo(config_with_branch())?;
    let root = repository.path();
    write_crate(root, ".velnor/cache/cargo/registry/src/untracked")?;
    write_crate(root, ".velnor/cache-extra")?;
    write_crate(root, ".velnor/cache2")?;
    write_crate(root, "src/generated/cache")?;

    let roots = selected_roots(root)?;
    assert!(
        roots.contains(&String::new()),
        "root crate must remain: {roots:?}"
    );
    assert!(
        roots.contains(&".velnor/cache-extra".to_owned()),
        "{roots:?}"
    );
    assert!(roots.contains(&".velnor/cache2".to_owned()), "{roots:?}");
    assert!(
        roots.contains(&"src/generated/cache".to_owned()),
        "{roots:?}"
    );
    assert!(
        !roots.iter().any(|path| path.starts_with(".velnor/cache/")),
        "reserved cache must be omitted: {roots:?}"
    );
    Ok(())
}

/// Filesystem-walk discovery applies the same exact reserved-root rule.
#[test]
fn non_git_discovery_excludes_only_reserved_cache_root() -> TestResult {
    let repository = make_repo(config_with_branch())?;
    let root = repository.path();
    fs::remove_dir_all(root.join(".git"))?;
    write_crate(root, ".velnor/cache/cargo/registry/src/untracked")?;
    write_crate(root, ".velnor/cache-extra")?;
    write_crate(root, "src/generated/cache")?;

    let roots = selected_roots(root)?;
    assert!(
        roots.contains(&String::new()),
        "root crate must remain: {roots:?}"
    );
    assert!(
        roots.contains(&".velnor/cache-extra".to_owned()),
        "{roots:?}"
    );
    assert!(
        roots.contains(&"src/generated/cache".to_owned()),
        "{roots:?}"
    );
    assert!(
        !roots.iter().any(|path| path.starts_with(".velnor/cache/")),
        "reserved cache must be omitted: {roots:?}"
    );
    Ok(())
}

/// A tracked non-UTF-8 descendant is rejected without matching siblings.
#[test]
#[cfg(unix)]
fn raw_non_utf8_tracked_cache_path_fails_public_discovery() -> TestResult {
    let repository = make_repo(config_with_branch())?;
    let root = repository.path();
    stage_raw_index_path(root, b".velnor/cache-extra/\xffpayload.crate")?;
    let prepared = prepare(root)?;
    assert!(
        prepared.discovery.skipped_non_utf8,
        "undecodable sibling is reported"
    );
    stage_raw_index_path(root, b".velnor/cache/\xffpayload.crate")?;

    let error = prepare(root).expect_err("raw tracked cache path must fail closed");
    assert!(
        error
            .to_string()
            .contains("tracked_reserved_cache_path:.velnor/cache/"),
        "unexpected error: {error}"
    );
    Ok(())
}

/// A stale Git path through a cache-parent alias is pruned without hiding siblings.
#[test]
#[cfg(unix)]
fn git_parent_alias_into_cache_is_pruned_but_sibling_is_discovered() -> TestResult {
    let repository = make_repo(config_with_branch())?;
    let root = repository.path();
    write_crate(root, "alias/private")?;
    write_crate(root, "alias/public")?;
    git(&["add", "alias"], root)?;

    fs::remove_dir_all(root.join("alias"))?;
    write_crate(root, "data/private")?;
    write_crate(root, "data/public")?;
    std::os::unix::fs::symlink("../data/private", root.join(".velnor/cache"))?;
    std::os::unix::fs::symlink("data", root.join("alias"))?;

    let roots = selected_roots(root)?;
    assert!(
        !roots.contains(&"alias/private".to_owned()),
        "cache target must be excluded: {roots:?}"
    );
    assert!(
        roots.contains(&"alias/public".to_owned()),
        "public sibling must remain selected: {roots:?}"
    );
    Ok(())
}

/// Add a staged index entry whose final path bytes are not UTF-8.
#[cfg(unix)]
fn stage_raw_index_path(root: &Path, path: &[u8]) -> TestResult {
    use std::ffi::OsString;
    use std::io::Write;
    use std::os::unix::ffi::OsStringExt;
    use std::process::{Command, Stdio};

    let mut command = Command::new("git");
    command
        .args(["hash-object", "-w", "-t", "blob", "--stdin"])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    let mut child = command.spawn()?;
    child
        .stdin
        .take()
        .ok_or("git hash-object stdin missing")?
        .write_all(b"payload")?;
    let output = child.wait_with_output()?;
    assert!(output.status.success(), "hash-object failed");
    let object = String::from_utf8(output.stdout)?;
    let mut cache_info = format!("100644,{},", object.trim()).into_bytes();
    cache_info.extend_from_slice(path);
    let output = Command::new("git")
        .args(["update-index", "--add", "--cacheinfo"])
        .arg(OsString::from_vec(cache_info))
        .current_dir(root)
        .output()?;
    assert!(
        output.status.success(),
        "update-index failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

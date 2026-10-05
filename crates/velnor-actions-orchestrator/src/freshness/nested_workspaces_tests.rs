use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;

use super::super::workspace_discovery::discover_with_limit;
use super::*;

const DAY: i64 = 86_400_000_000;

fn write(root: &Path, relative: &str, contents: &str) -> std::io::Result<()> {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, contents)
}

fn recursive_workspace_fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("workspace fixture");
    write(
        root.path(),
        "Cargo.toml",
        "[package]\nname = \"root\"\nversion = \"0.1.0\"\n\
         [workspace]\nmembers = []\nexclude = [\"tools/**\"]\n",
    )
    .expect("root workspace manifest");
    write(
        root.path(),
        "Cargo.lock",
        "version = 4\n[[package]]\nname = \"root\"\nversion = \"0.1.0\"\n",
    )
    .expect("root lock");
    write(root.path(), "deny.toml", "[advisories]\nignore = []\n").expect("root deny policy");
    write(
        root.path(),
        "tools/runner/Cargo.toml",
        "[package]\nname = \"runner\"\nversion = \"0.1.0\"\n\
         [workspace]\nmembers = [\"lib/[ab]\"]\nexclude = [\"inner\"]\n",
    )
    .expect("nested workspace manifest");
    for (name, directory) in [("a", "a"), ("b", "b"), ("c", "c")] {
        write(
            root.path(),
            &format!("tools/runner/lib/{directory}/Cargo.toml"),
            &format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n"),
        )
        .expect("member manifest");
    }
    write(
        root.path(),
        "tools/runner/Cargo.lock",
        "version = 4\n\
         [[package]]\nname = \"runner\"\nversion = \"0.1.0\"\n\
         [[package]]\nname = \"a\"\nversion = \"0.1.0\"\n\
         [[package]]\nname = \"b\"\nversion = \"0.1.0\"\n",
    )
    .expect("nested lock");
    write(
        root.path(),
        "tools/runner/deny.toml",
        "[advisories]\nignore = []\n",
    )
    .expect("nested deny policy");
    write(
        root.path(),
        "tools/runner/inner/Cargo.toml",
        "[package]\nname = \"inner\"\nversion = \"0.1.0\"\n\
         [workspace]\nmembers = []\n",
    )
    .expect("recursive nested workspace manifest");
    write(
        root.path(),
        "tools/runner/inner/Cargo.lock",
        "version = 4\n[[package]]\nname = \"inner\"\nversion = \"0.1.0\"\n",
    )
    .expect("recursive nested workspace lock");
    write(
        root.path(),
        "tools/runner/inner/deny.toml",
        "[advisories]\nignore = []\n",
    )
    .expect("recursive nested deny policy");
    root
}

#[test]
fn recursive_excludes_and_member_globs_include_root_packages() {
    let root = recursive_workspace_fixture();
    let mut ctx = FreshnessContext::new(root.path(), false, false);
    check_all(&mut ctx);
    assert!(ctx.failures.is_empty(), "{:?}", ctx.failures);
    assert_eq!(ctx.workspace_roots.len(), 3);
    assert_eq!(ctx.workspace_roots[0], Path::new(""));
    assert!(ctx.workspace_roots.contains(&PathBuf::from("tools/runner")));
    assert!(
        ctx.workspace_roots
            .contains(&PathBuf::from("tools/runner/inner"))
    );
    assert!(ctx.member_names.contains("root"));
    assert!(ctx.member_names.contains("runner"));
    assert!(ctx.member_names.contains("a"));
    assert!(ctx.member_names.contains("b"));
    assert!(ctx.member_names.contains("inner"));
    assert!(!ctx.member_names.contains("c"));
    assert_eq!(ctx.locked.as_ref().map(Vec::len), Some(5));
}

#[test]
fn nested_only_lock_packages_are_known_exception_subjects() {
    let root = recursive_workspace_fixture();
    let mut ctx = FreshnessContext::new(root.path(), false, false);
    ctx.now_micros = super::super::context::days_from_civil(2026, 10, 5) * DAY;
    check_all(&mut ctx);
    ctx.inv = json!({"exceptions": []});
    ctx.holds = vec![json!({
        "key": "runner",
        "held_version": "0.1.0",
        "owner": "release",
        "issue": "#12",
        "reason": "fixture hold",
        "granted": "2026-10-05",
        "expires": "2026-10-06"
    })];
    super::super::exceptions::check_exceptions(&mut ctx);
    assert!(
        !ctx.failures
            .iter()
            .any(|row| row.contains("hold subject matches no")),
        "nested lock package was omitted from exception context: {:?}",
        ctx.failures
    );
}

#[test]
fn missing_nested_deny_policy_fails_closed() {
    let root = recursive_workspace_fixture();
    fs::remove_file(root.path().join("tools/runner/deny.toml")).expect("remove nested deny");
    let mut ctx = FreshnessContext::new(root.path(), false, false);
    check_all(&mut ctx);
    check_advisory_policies(&mut ctx);
    assert!(
        ctx.failures
            .iter()
            .any(|row| row.contains("tools/runner/deny.toml")),
        "missing nested deny was not rejected: {:?}",
        ctx.failures
    );
}

#[test]
fn missing_root_workspace_fails_dependencies_and_advisory_discovery() {
    let root = tempfile::tempdir().expect("workspace fixture");
    write(
        root.path(),
        "Cargo.toml",
        "[package]\nname = \"standalone\"\nversion = \"0.1.0\"\n",
    )
    .expect("package manifest");
    let mut ctx = FreshnessContext::new(root.path(), false, false);
    check_all(&mut ctx);
    check_advisory_policies(&mut ctx);
    assert!(
        ctx.failures
            .iter()
            .any(|row| row.contains("no [workspace] table"))
    );
    assert!(
        ctx.failures
            .iter()
            .any(|row| row.contains("no Cargo workspaces discovered"))
    );
}

#[test]
fn malformed_workspace_members_fail_instead_of_becoming_empty() {
    let root = tempfile::tempdir().expect("workspace fixture");
    write(
        root.path(),
        "Cargo.toml",
        "[workspace]\nmembers = \"crates/*\"\n",
    )
    .expect("workspace manifest");
    write(root.path(), "Cargo.lock", "version = 4\n").expect("workspace lock");
    let mut ctx = FreshnessContext::new(root.path(), false, false);
    check_all(&mut ctx);
    assert!(
        ctx.failures
            .iter()
            .any(|row| row.contains("workspace.members must be an array"))
    );
}

#[test]
fn malformed_excludes_and_missing_member_globs_fail_closed() {
    let root = tempfile::tempdir().expect("workspace fixture");
    write(
        root.path(),
        "Cargo.toml",
        "[package]\nname = \"root\"\nversion = \"0.1.0\"\n\
         [workspace]\nmembers = [\"missing/*\"]\nexclude = [4]\n",
    )
    .expect("workspace manifest");
    write(
        root.path(),
        "Cargo.lock",
        "version = 4\n[[package]]\nname = \"root\"\nversion = \"0.1.0\"\n",
    )
    .expect("workspace lock");
    let mut ctx = FreshnessContext::new(root.path(), false, false);
    check_all(&mut ctx);
    assert!(
        ctx.failures
            .iter()
            .any(|row| row.contains("workspace.exclude entry is not a string"))
    );
    assert!(
        ctx.failures
            .iter()
            .any(|row| row.contains("workspace member manifest not found"))
    );
}

#[cfg(unix)]
#[test]
fn workspace_discovery_deduplicates_in_root_symlink_alias_cycles() -> std::io::Result<()> {
    let root = tempfile::tempdir()?;
    write(
        root.path(),
        "Cargo.toml",
        "[package]\nname = \"root\"\nversion = \"0.1.0\"\n\
         [workspace]\nmembers = []\nexclude = [\"a\", \"b\"]\n",
    )?;
    write(
        root.path(),
        "Cargo.lock",
        "version = 4\n[[package]]\nname = \"root\"\nversion = \"0.1.0\"\n",
    )?;
    write(root.path(), "deny.toml", "[advisories]\nignore = []\n")?;
    std::os::unix::fs::symlink(".", root.path().join("a"))?;
    std::os::unix::fs::symlink(".", root.path().join("b"))?;

    let mut ctx = FreshnessContext::new(root.path(), false, false);
    check_all(&mut ctx);

    assert!(ctx.failures.is_empty(), "{:?}", ctx.failures);
    assert_eq!(ctx.workspace_roots, [PathBuf::new()]);
    assert_eq!(ctx.locked.as_ref().map(Vec::len), Some(1));
    Ok(())
}

#[cfg(unix)]
#[test]
fn workspace_discovery_rejects_symlink_aliases_outside_repository_root() -> std::io::Result<()> {
    let root = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    write(
        root.path(),
        "Cargo.toml",
        "[package]\nname = \"root\"\nversion = \"0.1.0\"\n\
         [workspace]\nmembers = []\nexclude = [\"outside\"]\n",
    )?;
    write(
        root.path(),
        "Cargo.lock",
        "version = 4\n[[package]]\nname = \"root\"\nversion = \"0.1.0\"\n",
    )?;
    write(root.path(), "deny.toml", "[advisories]\nignore = []\n")?;
    write(outside.path(), "Cargo.toml", "[workspace]\nmembers = []\n")?;
    std::os::unix::fs::symlink(outside.path(), root.path().join("outside"))?;

    let mut ctx = FreshnessContext::new(root.path(), false, false);
    check_all(&mut ctx);

    assert!(
        ctx.failures
            .iter()
            .any(|row| row.contains("workspace directory resolves outside repository root")),
        "outside workspace alias was accepted: {:?}",
        ctx.failures
    );
    Ok(())
}

#[test]
fn workspace_discovery_has_a_global_unique_directory_bound() -> std::io::Result<()> {
    let root = tempfile::tempdir()?;
    write(
        root.path(),
        "Cargo.toml",
        "[package]\nname = \"root\"\nversion = \"0.1.0\"\n\
         [workspace]\nmembers = []\nexclude = [\"nested/*\"]\n",
    )?;
    for name in ["a", "b", "c"] {
        write(
            root.path(),
            &format!("nested/{name}/Cargo.toml"),
            &format!(
                "[workspace]\nmembers = []\n[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n"
            ),
        )?;
    }

    let mut ctx = FreshnessContext::new(root.path(), false, false);
    let workspaces = discover_with_limit(&mut ctx, 2);

    assert_eq!(workspaces.len(), 1);
    assert!(
        ctx.failures
            .iter()
            .any(|row| row.contains("workspace discovery exceeds 2 unique paths")),
        "discovery cap did not fail closed: {:?}",
        ctx.failures
    );
    Ok(())
}

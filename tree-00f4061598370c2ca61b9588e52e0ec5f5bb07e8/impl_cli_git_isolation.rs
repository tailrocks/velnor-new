//! Fixture mutations cannot borrow another repository's process authority.

use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use super::{cleanup, commit_all, fresh_tempdir, git_fixture, git_init, head_sha};

const CHILD: &str = "VELNOR_FIXTURE_ISOLATION_CHILD";
const ROOT: &str = "VELNOR_FIXTURE_ISOLATION_ROOT";

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        cleanup(&self.0);
    }
}

type Snapshot = BTreeMap<PathBuf, (Vec<u8>, SystemTime)>;

fn snapshot(root: &Path) -> Result<Snapshot, Box<dyn Error>> {
    let mut result = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory)? {
            let path = entry?.path();
            let metadata = std::fs::symlink_metadata(&path)?;
            if metadata.is_dir() {
                pending.push(path);
            } else if metadata.is_file() {
                result.insert(
                    path.strip_prefix(root)?.to_path_buf(),
                    (std::fs::read(path)?, metadata.modified()?),
                );
            }
        }
    }
    Ok(result)
}

fn run_git(root: &Path, args: &[&str]) -> Result<(), Box<dyn Error>> {
    let output = git_fixture::command(root)?.args(args).output()?;
    assert!(output.status.success(), "{:?}", output.stderr);
    Ok(())
}

fn hostile_child(root: &Path, victim: &Path, poison: &Path) -> Result<Command, Box<dyn Error>> {
    let mut child = Command::new(std::env::current_exe()?);
    child
        .args([
            "impl_cli_tmp::isolation_tests::fixture_git_ignores_inherited_authority",
            "--exact",
        ])
        .env(CHILD, "1")
        .env(ROOT, root)
        .env("GIT_DIR", victim.join(".git"))
        .env("GIT_COMMON_DIR", victim.join(".git"))
        .env("GIT_WORK_TREE", victim)
        .env("GIT_INDEX_FILE", victim.join(".git/index"))
        .env("GIT_OBJECT_DIRECTORY", victim.join(".git/objects"))
        .env(
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            victim.join(".git/objects"),
        )
        .env("GIT_CONFIG", victim.join(".git/config"))
        .env("GIT_CONFIG_SYSTEM", poison.join("config"))
        .env("GIT_CONFIG_GLOBAL", poison.join("config"))
        .env("GIT_TEMPLATE_DIR", poison.join("template"))
        .env("GIT_CONFIG_COUNT", "2")
        .env("GIT_CONFIG_KEY_0", "core.worktree")
        .env("GIT_CONFIG_VALUE_0", victim)
        .env("GIT_CONFIG_KEY_1", "core.hooksPath")
        .env("GIT_CONFIG_VALUE_1", poison.join("hooks"))
        .env("GIT_CONFIG_PARAMETERS", "'commit.gpgsign=true'")
        .env("HOME", poison)
        .env("XDG_CONFIG_HOME", poison);
    Ok(child)
}

fn prepare_poison(root: &Path) -> Result<(), Box<dyn Error>> {
    for directory in ["hooks", "template/hooks"] {
        std::fs::create_dir_all(root.join(directory))?;
        let hook = root.join(directory).join("pre-commit");
        std::fs::write(&hook, "#!/bin/sh\nexit 97\n")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755))?;
        }
    }
    std::fs::write(
        root.join("config"),
        format!(
            "[core]\nhooksPath = {}\n[commit]\ngpgsign = true\n[init]\ntemplateDir = {}\n",
            root.join("hooks").display(),
            root.join("template").display()
        ),
    )?;
    Ok(())
}

#[test]
fn fixture_git_ignores_inherited_authority() -> Result<(), Box<dyn Error>> {
    if std::env::var_os(CHILD).is_some() {
        let root = PathBuf::from(std::env::var_os(ROOT).ok_or("missing fixture root")?);
        git_init(&root)?;
        std::fs::write(root.join("fixture.txt"), "fixture only\n")?;
        let commit = commit_all(&root)?;
        assert_eq!(head_sha(&root)?, commit);
        run_git(&root, &["diff", "--exit-code", "HEAD"])?;
        assert!(!root.join(".git/hooks/pre-commit").exists());
        return Ok(());
    }
    let scratch = Scratch(fresh_tempdir("git-authority")?);
    let victim = scratch.0.join("victim");
    let fixture = scratch.0.join("fixture");
    let poison = scratch.0.join("poison");
    for directory in [&victim, &fixture, &poison] {
        std::fs::create_dir_all(directory)?;
    }
    git_init(&victim)?;
    std::fs::write(victim.join("victim.txt"), "original\n")?;
    let victim_head = commit_all(&victim)?;
    std::fs::write(victim.join("victim.txt"), "staged\n")?;
    run_git(&victim, &["add", "victim.txt"])?;
    std::fs::write(victim.join("victim.txt"), "unstaged\n")?;
    prepare_poison(&poison)?;
    let before = snapshot(&victim)?;
    let output = hostile_child(&fixture, &victim, &poison)?.output()?;
    assert!(
        output.status.success(),
        "child failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    assert_eq!(
        before,
        snapshot(&victim)?,
        "victim files and metadata changed"
    );
    assert_eq!(head_sha(&victim)?, victim_head);
    assert_ne!(head_sha(&fixture)?, victim_head);
    Ok(())
}

#[test]
fn fixture_git_resolves_linked_worktree_metadata() -> Result<(), Box<dyn Error>> {
    let scratch = Scratch(fresh_tempdir("git-linked")?);
    let repository = scratch.0.join("repo");
    let linked = scratch.0.join("linked");
    std::fs::create_dir_all(&repository)?;
    git_init(&repository)?;
    std::fs::write(repository.join("file.txt"), "original\n")?;
    let head = commit_all(&repository)?;
    let output = git_fixture::command(&repository)?
        .args(["worktree", "add", "-b", "linked"])
        .arg(&linked)
        .output()?;
    assert!(output.status.success(), "{:?}", output.stderr);
    assert!(linked.join(".git").is_file());
    assert_eq!(head_sha(&linked)?, head);
    std::fs::write(linked.join("file.txt"), "linked only\n")?;
    assert_ne!(commit_all(&linked)?, head);
    assert_eq!(head_sha(&repository)?, head);
    assert_eq!(std::fs::read(repository.join("file.txt"))?, b"original\n");
    Ok(())
}

#[test]
fn malformed_fixture_metadata_pointer_fails_before_spawn() -> Result<(), Box<dyn Error>> {
    let scratch = Scratch(fresh_tempdir("git-invalid")?);
    std::fs::write(scratch.0.join(".git"), "foreign metadata\n")?;
    assert!(git_fixture::command(&scratch.0).is_err());
    Ok(())
}

#[test]
fn git_fixture_commands_require_shared_authority() -> Result<(), Box<dyn Error>> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("missing crates directory")?;
    let helper = crates.join("test_support/git_fixture.rs");
    let identity_reader = crates.join("velnor-actions-cli/examples/validate_commit_trailers.rs");
    let gate_reader = crates.join("velnor-actions-freshness/src/trailer_policy.rs");
    let gate_source = std::fs::read_to_string(&gate_reader)?;
    assert_eq!(
        gate_source.matches("Command::new(\"git\")").count(),
        1,
        "the freshness gate keeps one bounded local-identity reader"
    );
    let gate_function = gate_source
        .split_once("fn git_identity(")
        .and_then(|(_, rest)| {
            rest.split_once("\nfn parse_git_identity(")
                .map(|(body, _)| body)
        })
        .ok_or("missing gate local-identity Git reader")?;
    assert!(gate_function.contains("run_bounded("));
    assert!(gate_function.contains(".args([\"var\", variable])"));
    assert!(gate_function.contains(".current_dir(root)"));
    assert!(gate_function.contains("IDENTITY_OUTPUT_CAP"));
    assert!(gate_function.contains("IDENTITY_TIMEOUT"));
    let identity_source = std::fs::read_to_string(&identity_reader)?;
    assert_eq!(
        identity_source.matches("Command::new(\"git\")").count(),
        1,
        "only the checked local-identity reader may spawn Git directly"
    );
    let identity_function = identity_source
        .split_once("fn git_identity(")
        .and_then(|(_, rest)| {
            rest.split_once("\nfn parse_git_identity(")
                .map(|(body, _)| body)
        })
        .ok_or("missing local-identity Git reader")?;
    let identity_caller = identity_source
        .split_once("fn validate_local_identities_with_env(")
        .and_then(|(_, rest)| rest.split_once("\nfn git_identity(").map(|(body, _)| body))
        .ok_or("missing local-identity caller")?;
    assert!(identity_caller.contains("[(\"AUTHOR\", \"author\"), (\"COMMITTER\", \"committer\")]"));
    assert_eq!(identity_function.matches(".arg(").count(), 2);
    assert!(identity_function.contains(".arg(\"var\")"));
    assert!(identity_function.contains(".arg(format!(\"GIT_{kind}_IDENT\"))"));
    assert!(!identity_function.contains(".args("));
    assert!(identity_function.contains(".current_dir(root)"));
    assert!(identity_function.contains(".env(\"GIT_OPTIONAL_LOCKS\", \"0\")"));
    let mut pending = vec![crates.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory)? {
            let path = entry?.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|value| value == "rs")
                && path != helper
                && path != identity_reader
                && path != gate_reader
            {
                let source = std::fs::read_to_string(&path)?;
                let direct_git = ["Command::new(", "\"git\")"].concat();
                assert!(
                    !source.contains(&direct_git),
                    "{} bypasses fixture Git isolation",
                    path.display()
                );
            }
        }
    }
    Ok(())
}

//! Deterministic source-config mutation between owned preparation and execution.
#![cfg(any(target_os = "linux", target_os = "macos"))]

// Shared fixture exposes helpers used by other test modules.
#[expect(
    dead_code,
    unused_imports,
    reason = "fixture helpers are shared by sibling test modules"
)]
#[cfg(test)]
#[path = "../tests/impl_mise_git_owned_config_fixture.rs"]
mod fixture;

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::{
    Bounds, CancelHandle, Duration, Instant, IsolatedCommand, MiseError, Preparation,
    execute_owned, native, prepare_owned, read, source,
};
use fixture::{git_dir, git_owned, install_hook, repo, touch_identical};

const CAP: usize = 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Eq, PartialEq)]
struct Snapshot {
    bytes: Vec<u8>,
    modified: SystemTime,
    device: u64,
    inode: u64,
    links: u64,
}

fn snapshot(path: &Path) -> Result<Snapshot, String> {
    let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
    Ok(Snapshot {
        bytes: fs::read(path).map_err(|error| error.to_string())?,
        modified: metadata.modified().map_err(|error| error.to_string())?,
        device: metadata.dev(),
        inode: metadata.ino(),
        links: metadata.nlink(),
    })
}

#[derive(Clone, Copy)]
enum Layer {
    Nested,
    Home,
    Xdg,
    System,
}

fn owner(root: &Path, args: &[&str], layer: Layer) -> Result<IsolatedCommand, String> {
    let mut command =
        IsolatedCommand::direct("git", args.iter().map(|arg| (*arg).into()).collect());
    command.cwd = Some(root.to_path_buf());
    let home = root.join("caller-home");
    let xdg = root.join("caller-xdg");
    fs::create_dir_all(&home).map_err(|error| error.to_string())?;
    fs::create_dir_all(xdg.join("git")).map_err(|error| error.to_string())?;
    let mut command = command
        .with_env(&[
            ("HOME".into(), home.into_os_string()),
            ("XDG_CONFIG_HOME".into(), xdg.into_os_string()),
        ])
        .map_err(|error| error.to_string())?;
    if matches!(layer, Layer::System) {
        // Privileged private test owner: fixed fixture file, never public caller input.
        command.extra_env.push((
            "GIT_CONFIG_SYSTEM".into(),
            root.join("caller-system.config").into_os_string(),
        ));
    }
    Ok(command)
}

fn config_leaf(root: &Path, layer: Layer) -> Result<PathBuf, String> {
    let leaf = match layer {
        Layer::Home => root.join("caller-home/.gitconfig"),
        Layer::Xdg => root.join("caller-xdg/git/config"),
        Layer::System => root.join("caller-system.config"),
        Layer::Nested => {
            let parent = root.join("outer.inc");
            let leaf = root.join("inner.inc");
            fs::write(&parent, format!("[include]\n\tpath = {}\n", leaf.display()))
                .map_err(|error| error.to_string())?;
            git_owned(
                root,
                vec![
                    "config".to_owned(),
                    "--add".to_owned(),
                    "--".to_owned(),
                    "include.path".to_owned(),
                    parent.to_string_lossy().into_owned(),
                ],
            )?;
            leaf
        }
    };
    fs::write(
        &leaf,
        b"[remote \"origin\"]\n\turl = https://example.invalid/original\n",
    )
    .map_err(|error| error.to_string())?;
    Ok(leaf)
}

fn finish(
    source: source::PreparedSource,
    result: Result<super::super::process::Outcome, MiseError>,
) -> Result<Result<super::ProcessOutput, MiseError>, String> {
    let private_path = source.root().path().to_path_buf();
    if result
        .as_ref()
        .is_ok_and(|outcome| !outcome.safe_to_cleanup)
    {
        source.retain_unreaped();
        return Err("execution child unreaped; source root retained".to_owned());
    }
    source.finish().map_err(|error| error.to_string())?;
    assert!(
        !private_path.exists(),
        "private source root survived finish"
    );
    Ok(result.and_then(|outcome| outcome.result))
}

fn late_filter(layer: Layer, operation: &str) -> Result<(), String> {
    let fixture = repo(&format!("late-filter-{operation}"), "sha1", 4)?;
    let owner = owner(&fixture.root, &["diff", "--name-only"], layer)?;
    let leaf = config_leaf(&fixture.root, layer)?;
    let hook = install_hook(&fixture.root)?;
    fs::write(
        fixture.root.join(".gitattributes"),
        b"tracked.txt filter=hostile\n",
    )
    .map_err(|error| error.to_string())?;
    touch_identical(&fixture.root.join("tracked.txt"))?;
    let marker = fixture.root.join("filter.marker");
    let script = fixture.root.join("filter.sh");
    // Invocation through sh needs no executable permission or protocol handshake.
    fs::write(
        &script,
        format!("printf invoked > '{}'\ncat\n", marker.display()),
    )
    .map_err(|error| error.to_string())?;
    let local = git_dir(&fixture.root)?.join("config");
    let index = git_dir(&fixture.root)?.join("index");
    let local_before = snapshot(&local)?;
    let index_before = snapshot(&index)?;
    let invocation = read::prepare(&owner.args).map_err(|error| error.to_string())?;
    let cancel = CancelHandle::new();
    let native = native::NativeGitBinding::discover(&owner, CAP, TIMEOUT, &cancel)
        .map_err(|error| error.to_string())?;
    let source =
        source::PreparedSource::prepare(&owner, &invocation).map_err(|error| error.to_string())?;
    let bounds = Bounds {
        native: &native,
        cap: CAP,
        start: Instant::now(),
        timeout: TIMEOUT,
        cancel: &cancel,
    };
    let ready = match prepare_owned(&owner, &invocation, &source, &bounds)
        .map_err(|error| error.to_string())?
    {
        Preparation::Ready(ready) => *ready,
        Preparation::Failed(outcome) => {
            let failed = finish(source, Ok(outcome))?;
            return Err(format!("owned preparation failed: {failed:?}"));
        }
    };
    fs::write(
        &leaf,
        format!(
            "[filter \"hostile\"]\n\t{operation} = sh '{}'\n",
            script.display()
        ),
    )
    .map_err(|error| error.to_string())?;
    assert_eq!(local_before, snapshot(&local)?);
    let result = execute_owned(&owner, invocation, &source, ready, &bounds);
    let result = finish(source, result)?;
    assert!(
        matches!(result, Err(MiseError::InvalidStepInput { field, value })
        if field == "git_owned_config" && value == "native_config_filter_unsupported")
    );
    assert_eq!(local_before, snapshot(&local)?);
    assert_eq!(index_before, snapshot(&index)?);
    assert!(!marker.exists(), "filter delegate executed");
    assert!(!hook.exists(), "post-index hook executed");
    Ok(())
}

#[test]
fn late_nested_include_clean_refuses_after_owned_prepare() -> Result<(), String> {
    late_filter(Layer::Nested, "clean")
}

#[test]
fn late_nested_include_process_refuses_after_owned_prepare() -> Result<(), String> {
    late_filter(Layer::Nested, "process")
}

#[test]
fn late_home_global_clean_refuses_after_owned_prepare() -> Result<(), String> {
    late_filter(Layer::Home, "clean")
}

#[test]
fn late_home_global_process_refuses_after_owned_prepare() -> Result<(), String> {
    late_filter(Layer::Home, "process")
}

#[test]
fn late_xdg_global_clean_refuses_after_owned_prepare() -> Result<(), String> {
    late_filter(Layer::Xdg, "clean")
}

#[test]
fn late_xdg_global_process_refuses_after_owned_prepare() -> Result<(), String> {
    late_filter(Layer::Xdg, "process")
}

#[test]
fn late_private_system_fixture_clean_refuses_after_owned_prepare() -> Result<(), String> {
    late_filter(Layer::System, "clean")
}

#[test]
fn late_private_system_fixture_process_refuses_after_owned_prepare() -> Result<(), String> {
    late_filter(Layer::System, "process")
}

fn unchanged_global(layer: Layer) -> Result<(), String> {
    let fixture = repo("unchanged-caller-global", "sha1", 4)?;
    let owner = owner(
        &fixture.root,
        &["config", "--get", "remote.origin.url"],
        layer,
    )?;
    let leaf = config_leaf(&fixture.root, layer)?;
    let local = git_dir(&fixture.root)?.join("config");
    let index = git_dir(&fixture.root)?.join("index");
    let local_before = snapshot(&local)?;
    let index_before = snapshot(&index)?;
    let global_before = snapshot(&leaf)?;
    let invocation = read::prepare(&owner.args).map_err(|error| error.to_string())?;
    let cancel = CancelHandle::new();
    let native = native::NativeGitBinding::discover(&owner, CAP, TIMEOUT, &cancel)
        .map_err(|error| error.to_string())?;
    let source =
        source::PreparedSource::prepare(&owner, &invocation).map_err(|error| error.to_string())?;
    let bounds = Bounds {
        native: &native,
        cap: CAP,
        start: Instant::now(),
        timeout: TIMEOUT,
        cancel: &cancel,
    };
    let ready = match prepare_owned(&owner, &invocation, &source, &bounds)
        .map_err(|error| error.to_string())?
    {
        Preparation::Ready(ready) => *ready,
        Preparation::Failed(outcome) => {
            let failed = finish(source, Ok(outcome))?;
            return Err(format!("owned preparation failed: {failed:?}"));
        }
    };
    let result = execute_owned(&owner, invocation, &source, ready, &bounds);
    let output = finish(source, result)?.map_err(|error| error.to_string())?;
    assert!(output.success, "ordinary global config read failed");
    assert_eq!(output.stdout, b"https://example.invalid/original\n");
    assert!(output.stderr.is_empty());
    assert_eq!(global_before, snapshot(&leaf)?);
    assert_eq!(local_before, snapshot(&local)?);
    assert_eq!(index_before, snapshot(&index)?);
    Ok(())
}

#[test]
fn unchanged_home_and_xdg_globals_survive_owned_prepare_and_recapture() -> Result<(), String> {
    unchanged_global(Layer::Home)?;
    unchanged_global(Layer::Xdg)
}

#[test]
fn unchanged_private_system_fixture_survives_owned_prepare_and_recapture() -> Result<(), String> {
    unchanged_global(Layer::System)
}

#[test]
fn public_caller_cannot_override_system_config() {
    let command = IsolatedCommand::direct("git", vec!["diff".into(), "--name-only".into()]);
    let result = command.with_env(&[("GIT_CONFIG_SYSTEM".into(), "caller-system.config".into())]);
    assert!(
        matches!(result, Err(MiseError::InvalidStepInput { field, value })
        if field == "GIT_CONFIG_SYSTEM" && value == "reserved_env_key")
    );
}

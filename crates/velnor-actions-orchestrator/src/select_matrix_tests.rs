//! Affected-selection matrix over a real Cargo workspace and real Git.
//!
//! Fixture: two libraries (`core`, `util`), eight apps (`a1`..`a8`), and a
//! vendored crate `patched` reached only through the registry crate
//! `regcrate` (served offline from a directory source and redirected with
//! `[patch.crates-io]`). Both the `--no-deps` inventory and the full
//! `--locked --offline` resolve come from real `cargo metadata`, so the
//! registry-to-patch edge is exactly what Cargo reports. Each scenario
//! commits one change and asserts the selected package names, or that the
//! selection broadened to everything.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use velnor_actions_contract::WorkflowEvent;
use velnor_actions_rust::{
    CompileDriver, NextestProfile, ProfileSource, RustExecutionProfile, TestRunner,
    parse_metadata_json, parse_resolved_graph,
};

use super::classify_changed;
use crate::affected_inputs::{AffectedInputs, InputRule, WorkspaceResolve};
use crate::discover::{Discovery, PlannedWorkspace};

type Outcome<T> = Result<T, Box<dyn std::error::Error>>;

/// Write `body` to `root/path`, creating parents.
fn put(root: &Path, path: &str, body: &str) -> Outcome<()> {
    let file = root.join(path);
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(file, body)?;
    Ok(())
}

/// One package manifest with the given dependency tables.
fn manifest(name: &str, tables: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n{tables}")
}

/// Build the two-library, eight-app fixture workspace root.
fn write_fixture(root: &Path) -> Outcome<()> {
    let members = [
        "libs/core",
        "libs/util",
        "apps/a1",
        "apps/a2",
        "apps/a3",
        "apps/a4",
        "apps/a5",
        "apps/a6",
        "apps/a7",
        "apps/a8",
        "vendor/patched",
    ];
    let list = members
        .iter()
        .map(|member| format!("\"{member}\""))
        .collect::<Vec<_>>()
        .join(", ");
    put(
        root,
        "Cargo.toml",
        &format!(
            "[workspace]\nmembers = [{list}]\nresolver = \"2\"\n\n\
             [patch.crates-io]\npatched = {{ path = \"vendor/patched\" }}\n"
        ),
    )?;
    put(
        root,
        ".cargo/config.toml",
        "[source.crates-io]\nreplace-with = \"fixture\"\n\n[source.fixture]\ndirectory = \"registry\"\n",
    )?;
    put(
        root,
        "registry/regcrate-0.1.0/Cargo.toml",
        &manifest("regcrate", "[dependencies]\npatched = \"0.1\"\n"),
    )?;
    put(
        root,
        "registry/regcrate-0.1.0/src/lib.rs",
        "pub use patched::f;\n",
    )?;
    put(
        root,
        "registry/regcrate-0.1.0/.cargo-checksum.json",
        "{\"files\":{},\"package\":null}\n",
    )?;
    write_packages(root)
}

/// Write the libraries, apps, vendored crate, and their extra inputs.
fn write_packages(root: &Path) -> Outcome<()> {
    let core = "core = { path = \"../../libs/core\" }\n";
    let util = "util = { path = \"../../libs/util\" }\n";
    let packages: [(&str, &str, String); 11] = [
        ("libs/core", "core", String::new()),
        ("libs/util", "util", String::new()),
        ("apps/a1", "a1", format!("[dependencies]\n{core}")),
        ("apps/a2", "a2", format!("[dependencies]\n{util}")),
        ("apps/a3", "a3", format!("[dev-dependencies]\n{core}")),
        (
            "apps/a4",
            "a4",
            format!("build = \"build.rs\"\n[build-dependencies]\n{core}"),
        ),
        (
            "apps/a5",
            "a5",
            format!("[target.'cfg(unix)'.dependencies]\n{core}"),
        ),
        (
            "apps/a6",
            "a6",
            "[features]\nextra = [\"dep:core\"]\n[dependencies]\ncore = { path = \"../../libs/core\", optional = true }\n"
                .to_owned(),
        ),
        (
            "apps/a7",
            "a7",
            "[dependencies]\nregcrate = \"0.1\"\n".to_owned(),
        ),
        ("apps/a8", "a8", String::new()),
        ("vendor/patched", "patched", String::new()),
    ];
    for (dir, name, tables) in &packages {
        put(root, &format!("{dir}/Cargo.toml"), &manifest(name, tables))?;
        put(root, &format!("{dir}/src/lib.rs"), "pub fn f() {}\n")?;
    }
    put(root, "apps/a4/build.rs", "fn main() {}\n")?;
    put(root, "libs/core/data/schema.sql", "select 1;\n")?;
    put(root, "libs/util/fixtures/case.json", "{}\n")?;
    put(root, "protos/v1/a8.proto", "syntax = \"proto3\";\n")?;
    put(root, "docs/notes.md", "notes\n")?;
    #[cfg(unix)]
    std::os::unix::fs::symlink("../../libs/util/fixtures", root.join("apps/a1/fixtures"))?;
    run(
        Command::new("cargo").args(["generate-lockfile", "--offline"]),
        root,
    )?;
    Ok(())
}

/// Run one command in `root`, returning stdout; failure is an error.
fn run(command: &mut Command, root: &Path) -> Outcome<String> {
    let output = command.current_dir(root).output()?;
    if !output.status.success() {
        return Err(format!("{command:?}: {}", String::from_utf8_lossy(&output.stderr)).into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

/// Run fixture Git in `root`.
fn git(root: &Path, args: &[&str]) -> Outcome<String> {
    run(crate::git_fixture::command(root)?.args(args), root)
}

/// Commit everything and return `HEAD`.
fn commit(root: &Path, message: &str) -> Outcome<String> {
    git(root, &["add", "-A"])?;
    git(root, &["commit", "-q", "-m", message])?;
    Ok(git(root, &["rev-parse", "HEAD"])?.trim().to_owned())
}

/// Discovery from real `cargo metadata`: inventory, resolve, and rules.
fn discovery(root: &Path) -> Outcome<Discovery> {
    let manifest = root.join("Cargo.toml");
    let manifest = manifest.to_string_lossy();
    let shallow = run(
        Command::new("cargo").args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
            &manifest,
        ]),
        root,
    )?;
    let full = run(
        Command::new("cargo").args([
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--offline",
            "--all-features",
            "--manifest-path",
            &manifest,
        ]),
        root,
    )?;
    let record = parse_metadata_json(&shallow, root, "Cargo.toml", &BTreeSet::new())?;
    let graph = parse_resolved_graph(&full, root, "Cargo.toml")?;
    let id = |name: &str| -> Outcome<String> {
        record
            .packages
            .iter()
            .find(|package| package.name == name)
            .map(|package| package.id.clone())
            .ok_or_else(|| format!("missing package {name}").into())
    };
    let rules = vec![
        InputRule {
            globs: vec!["protos/**/*.proto".to_owned()],
            package_ids: BTreeSet::from([id("a8")?]),
        },
        InputRule {
            globs: vec!["libs/core/data/**".to_owned()],
            package_ids: BTreeSet::from([id("a8")?]),
        },
    ];
    Ok(Discovery {
        workspaces: vec![PlannedWorkspace {
            record,
            profile: RustExecutionProfile {
                compile_driver: CompileDriver::Cargo,
                test_runner: TestRunner::CargoTest,
                evidence: Vec::new(),
                driver_source: ProfileSource::Detected,
                runner_source: ProfileSource::Detected,
                nextest_profile: NextestProfile::Default,
                nextest_config: None,
                run_ignored: None,
            },
            recommendations: Vec::new(),
            findings: Vec::new(),
        }],
        affected: AffectedInputs {
            resolves: vec![WorkspaceResolve {
                workspace_root: String::new(),
                graph: Ok(graph),
            }],
            rules,
        },
        ..Discovery::default()
    })
}

/// Selected package names, or `None` when the selection broadened.
fn select(root: &Path, base: &str, head: &str) -> Outcome<Option<BTreeSet<String>>> {
    let discovery = discovery(root)?;
    let names: BTreeMap<String, String> = discovery.workspaces[0]
        .record
        .packages
        .iter()
        .map(|package| (package.id.clone(), package.name.clone()))
        .collect();
    let mut warnings = Vec::new();
    let changed = classify_changed(
        root,
        WorkflowEvent::PullRequest,
        Some(base),
        head,
        &discovery,
        &mut warnings,
    );
    if warnings
        .iter()
        .any(|w| w.ends_with(":all_changed") || w.ends_with(":selecting_all"))
    {
        return Ok(None);
    }
    let changed = changed.ok_or("unknown comparison without a broadening warning")?;
    Ok(Some(
        changed
            .iter()
            .filter_map(|id| names.get(id).cloned())
            .collect(),
    ))
}

/// Fresh committed fixture plus its base commit.
fn fixture() -> Outcome<(tempfile::TempDir, PathBuf, String)> {
    let dir = tempfile::tempdir()?;
    let root = dir.path().canonicalize()?;
    run(
        crate::git_fixture::sterile_command().args(["init", "-q", "-b", "main"]),
        &root,
    )?;
    write_fixture(&root)?;
    let base = commit(&root, "base")?;
    Ok((dir, root, base))
}

/// Apply one edit to a fresh fixture and return the selection.
fn scenario(edit: impl FnOnce(&Path) -> Outcome<()>) -> Outcome<Option<BTreeSet<String>>> {
    let (_dir, root, base) = fixture()?;
    edit(&root)?;
    let head = commit(&root, "change")?;
    select(&root, &base, &head)
}

fn names(list: &[&str]) -> BTreeSet<String> {
    list.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn library_used_by_one_app_selects_exactly_two_packages() -> Outcome<()> {
    let selected = scenario(|root| put(root, "libs/util/src/lib.rs", "pub fn g() {}\n"))?;
    // `a1` reads `libs/util/fixtures` through its tracked symlink, but a
    // source edit outside the linked directory stays with `util`'s users.
    assert_eq!(selected.as_ref(), Some(&names(&["a2", "util"])));
    Ok(())
}

#[test]
fn shared_library_selects_every_dependency_kind() -> Outcome<()> {
    let selected = scenario(|root| put(root, "libs/core/src/lib.rs", "pub fn g() {}\n"))?;
    assert_eq!(
        selected.as_ref(),
        Some(&names(&["a1", "a3", "a4", "a5", "a6", "core"]))
    );
    Ok(())
}

#[test]
fn one_app_selects_only_itself() -> Outcome<()> {
    let selected = scenario(|root| put(root, "apps/a1/src/lib.rs", "pub fn g() {}\n"))?;
    assert_eq!(selected.as_ref(), Some(&names(&["a1"])));
    Ok(())
}

#[test]
fn lockfile_change_selects_everything() -> Outcome<()> {
    let selected = scenario(|root| {
        let lock = fs::read_to_string(root.join("Cargo.lock"))?;
        fs::write(root.join("Cargo.lock"), format!("{lock}\n"))?;
        Ok(())
    })?;
    assert_eq!(selected, None);
    Ok(())
}

#[test]
fn unowned_file_selects_everything() -> Outcome<()> {
    let selected = scenario(|root| put(root, "docs/notes.md", "changed\n"))?;
    assert_eq!(selected, None);
    Ok(())
}

#[test]
fn declared_proto_input_selects_its_consumer() -> Outcome<()> {
    let selected = scenario(|root| put(root, "protos/v1/a8.proto", "syntax = \"proto3\"; // x\n"))?;
    assert_eq!(selected.as_ref(), Some(&names(&["a8"])));
    Ok(())
}

#[test]
fn cross_crate_declared_input_selects_owner_and_reader() -> Outcome<()> {
    let selected = scenario(|root| put(root, "libs/core/data/schema.sql", "select 2;\n"))?;
    assert_eq!(
        selected.as_ref(),
        Some(&names(&["a1", "a3", "a4", "a5", "a6", "a8", "core"]))
    );
    Ok(())
}

#[test]
fn vendored_patch_crate_selects_registry_dependents() -> Outcome<()> {
    let selected = scenario(|root| put(root, "vendor/patched/src/lib.rs", "pub fn f() {} //\n"))?;
    assert_eq!(selected.as_ref(), Some(&names(&["a7", "patched"])));
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlinked_fixture_selects_the_reading_app() -> Outcome<()> {
    let selected = scenario(|root| put(root, "libs/util/fixtures/case.json", "{\"x\":1}\n"))?;
    assert_eq!(selected.as_ref(), Some(&names(&["a1", "a2", "util"])));
    Ok(())
}

#[test]
fn scoped_clippy_config_selects_members_below() -> Outcome<()> {
    let selected = scenario(|root| put(root, "apps/clippy.toml", "msrv = \"1.80\"\n"))?;
    assert_eq!(
        selected.as_ref(),
        Some(&names(&["a1", "a2", "a3", "a4", "a5", "a6", "a7", "a8"]))
    );
    Ok(())
}

#[test]
fn root_cargo_config_selects_everything() -> Outcome<()> {
    let selected = scenario(|root| {
        let config = fs::read_to_string(root.join(".cargo/config.toml"))?;
        fs::write(root.join(".cargo/config.toml"), format!("{config}# edit\n"))?;
        Ok(())
    })?;
    assert_eq!(selected, None);
    Ok(())
}

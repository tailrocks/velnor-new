//! Cargo inventory reuse + tool-drift tests.
//!
//! Declared via `#[path]` from `inventory.rs` under `cfg(test)` so the
//! inventory module keeps the file size gate.

use std::cell::Cell;

use velnor_actions_rust::PackageRecord;

use super::*;

#[test]
fn tool_missing_markers_are_conservative() {
    assert!(is_tool_missing("mise ERROR Tool rust@1.2.3 not installed"));
    assert!(is_tool_missing("No such tool: nextest"));
    assert!(!is_tool_missing("error: bad manifest"));
    assert!(!is_tool_missing(""));
}

fn package(manifest: &str) -> PackageRecord {
    PackageRecord {
        id: format!("pkg {manifest}"),
        name: "pkg".to_owned(),
        version: "0.1.0".to_owned(),
        manifest: manifest.to_owned(),
        external: false,
        in_workspace: true,
        targets: Vec::new(),
        features: Vec::new(),
        has_build_script: false,
    }
}

fn record(workspace_root: &str, manifests: &[&str]) -> WorkspaceRecord {
    WorkspaceRecord {
        workspace_root: workspace_root.to_owned(),
        members: manifests.iter().map(|m| format!("pkg {m}")).collect(),
        packages: manifests.iter().map(|m| package(m)).collect(),
        edges: Vec::new(),
        skipped_edges: Vec::new(),
    }
}

fn candidates(manifests: &[String]) -> Vec<String> {
    manifests.to_vec()
}

fn fixture_dir(files: &[(&str, &str)]) -> Result<tempfile::TempDir, String> {
    let dir = tempfile::TempDir::new().map_err(|err| err.to_string())?;
    for (path, body) in files {
        let full = dir.path().join(path);
        let parent = full.parent().ok_or("fixture path lacks a parent")?;
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        std::fs::write(&full, body).map_err(|err| err.to_string())?;
    }
    Ok(dir)
}

const MEMBER: &str = "[package]\nname = \"x\"\nversion = \"0.1.0\"\n";
const NESTED_ROOT: &str = "[package]\nname = \"n\"\nversion = \"0.1.0\"\n[workspace]\n";

/// Reuse matches legacy outcomes/inventories; fetches drop 6 to 4.
#[test]
fn reuse_matches_legacy_outcomes_and_inventories() -> Result<(), String> {
    let head = ["Cargo.toml", "crates/a/Cargo.toml", "crates/b/Cargo.toml"];
    let tail = ["nested/Cargo.toml", "other/Cargo.toml", "bad/Cargo.toml"];
    let names: Vec<&str> = head.into_iter().chain(tail).collect();
    let mut files: Vec<(&str, &str)> = names.iter().map(|n| (*n, MEMBER)).collect();
    files[3] = (names[3], NESTED_ROOT);
    let dir = fixture_dir(&files)?;
    let outer = record("", &[names[0], names[1], names[2], names[3]]);
    let nested = record("nested", &names[3..4]);
    let other = record("other", &names[4..5]);
    let load = |manifest: &str| match manifest {
        "Cargo.toml" | "crates/a/Cargo.toml" | "crates/b/Cargo.toml" => Ok(outer.clone()),
        "nested/Cargo.toml" => Ok(nested.clone()),
        "other/Cargo.toml" => Ok(other.clone()),
        "bad/Cargo.toml" => Err(FetchFailure::Malformed("bad toml".to_owned())),
        unexpected => Err(FetchFailure::Malformed(format!("unexpected:{unexpected}"))),
    };
    let manifests: Vec<String> = names.iter().map(ToString::to_string).collect();
    let run = |reuse: bool| {
        let calls = Cell::new(0_usize);
        let counting = |manifest: &str| {
            calls.set(calls.get() + 1);
            load(manifest)
        };
        let result = run_with(dir.path(), &candidates(&manifests), reuse, &counting);
        result.map(|inventories| (inventories, calls.get()))
    };
    let (legacy, legacy_calls) = run(false).map_err(|err| format!("{err:?}"))?;
    let (reused, reuse_calls) = run(true).map_err(|err| format!("{err:?}"))?;
    assert_eq!(legacy_calls, 6, "legacy loads every candidate");
    assert_eq!(reuse_calls, 4, "members reuse the root record");
    assert_eq!(legacy.0, reused.0, "outcomes match");
    assert_eq!(legacy.1, reused.1, "inventories match");
    let bad = &reused.0[5];
    assert!(!bad.metadata_ok, "malformed preserved");
    assert_eq!(bad.manifest, "bad/Cargo.toml");
    for outcome in &reused.0 {
        if outcome.manifest != "bad/Cargo.toml" {
            assert!(outcome.diagnostic.is_none(), "clean {}", outcome.manifest);
        }
    }
    let nested = &reused.1[3].1;
    assert_eq!(nested.workspace_root, "nested");
    Ok(())
}

/// A tool write mid-fetch fails the run instead of slipping through.
#[test]
fn tool_mutation_mid_run_fails_closed() -> Result<(), String> {
    let dir = fixture_dir(&[("Cargo.toml", MEMBER), ("mise.toml", "v1")])?;
    let one = candidates(&["Cargo.toml".to_owned()]);
    let loader = |manifest: &str| {
        std::fs::write(dir.path().join("mise.toml"), "v2").expect("mutate tool");
        Ok(record("", &[manifest]))
    };
    match run_with(dir.path(), &one, true, &loader) {
        Err(OrchestratorError::Contract { problem })
            if problem == "tool_files_changed:mise.toml" => {}
        Err(other) => return Err(format!("wrong error: {other:?}")),
        Ok(_) => return Err("expected tool drift failure".to_owned()),
    }
    Ok(())
}

/// Incomplete fetches abort both paths with the same error.
#[test]
fn incomplete_aborts_both_paths() -> Result<(), String> {
    let dir = fixture_dir(&[("Cargo.toml", MEMBER)])?;
    let manifests = ["Cargo.toml".to_owned()];
    let failed = |_: &str| Err(FetchFailure::Incomplete("metadata_offline:x".to_owned()));
    for reuse in [false, true] {
        let result = run_with(dir.path(), &candidates(&manifests), reuse, &failed);
        match result {
            Err(OrchestratorError::PreparationIncomplete { manifest, problem })
                if manifest == "Cargo.toml" && problem == "metadata_offline:x" => {}
            Err(other) => return Err(format!("wrong error: {other:?}")),
            Ok(_) => return Err("expected preparation_incomplete".to_owned()),
        }
    }
    Ok(())
}

/// Subprocess counts: N+1 legacy, exactly 1 reused; the P13 measurements.
#[test]
fn metadata_subprocess_counts() -> Result<(), String> {
    for members in [1_usize, 10, 100] {
        let mut names = vec!["Cargo.toml".to_owned()];
        for index in 0..members {
            names.push(format!("crates/c{index:03}/Cargo.toml"));
        }
        let files: Vec<(&str, &str)> = names.iter().map(|n| (n.as_str(), MEMBER)).collect();
        let dir = fixture_dir(&files)?;
        let listed: Vec<&str> = names.iter().map(String::as_str).collect();
        let workspace = record("", &listed);
        let count = |reuse: bool| {
            let calls = Cell::new(0_usize);
            let load = |_: &str| {
                calls.set(calls.get() + 1);
                Ok(workspace.clone())
            };
            assert!(run_with(dir.path(), &candidates(&names), reuse, &load).is_ok());
            calls.get()
        };
        assert_eq!(count(false), members + 1, "legacy scales with members");
        assert_eq!(count(true), 1, "reuse loads once per workspace");
    }
    Ok(())
}

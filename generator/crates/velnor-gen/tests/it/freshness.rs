// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::notes;
use velnor_model::{RepoPath, RepoView, Target, TargetKind, TargetName};
use velnor_rust::silent_build_scripts;

use std::fs;
use std::os::unix::fs::symlink;
use std::process::Command;

use crate::facts::TestResult;
use crate::lock::locked;
use crate::pipeline::{view, workflow};

const DEBUG_LOG: &str = "CARGO_LOG: '${{ vars.VELNOR_CACHE_DEBUG == ''1'' && ''cargo::compiler::fingerprint=info,cargo::core::compiler::fingerprint=info'' || '''' }}'";

fn steps(text: &str) -> Vec<&str> {
    text.split("\n      - ").skip(1).collect()
}

fn named<'t>(text: &'t str, name: &str) -> Vec<&'t str> {
    steps(text)
        .into_iter()
        .filter(|step| {
            step.lines()
                .any(|line| line.trim_start_matches("- ").trim() == format!("name: {name}"))
        })
        .collect()
}

fn names_of(lane: &str) -> Vec<&str> {
    lane.lines()
        .filter(|line| line.starts_with("      - ") || line.starts_with("        name: "))
        .filter_map(|line| line.trim().trim_start_matches("- ").strip_prefix("name: "))
        .collect()
}

#[test]
fn every_lane_restores_the_file_times_of_its_root_between_the_caches_and_the_restored_step()
-> TestResult {
    let mut repo = locked("0.1.82")?;
    repo.files.insert(
        RepoPath::new(".velnor/rust-ci.toml")?,
        b"full_run = \"nightly\"\n".to_vec(),
    );
    let more = view(&["Cargo.toml", "lab/Cargo.toml"], None)?;
    repo.rust_metadata = more.rust_metadata;
    repo.tracked_paths.extend(more.tracked_paths);
    let text = workflow(&repo)?;
    for lane in ["rust-root", "rust-lab"] {
        let header = format!("  {lane}:\n");
        let from = text.find(&header).ok_or("lane missing")? + header.len();
        let rest = &text[from..];
        let end = rest
            .find("\n  rust-")
            .or_else(|| rest.find("\n  required:"));
        let order: Vec<&str> = names_of(end.map_or(rest, |end| &rest[..end]))
            .into_iter()
            .filter(|name| *name != "Verify the released helper")
            .collect();
        let restored = order
            .iter()
            .position(|name| *name == "velnor-ci restored")
            .ok_or("no restored step")?;
        assert_eq!(order[restored - 2], "velnor-ci delta apply", "{lane}");
        assert_eq!(order[restored - 1], "velnor-ci inputs restore", "{lane}");
        assert_eq!(order[restored + 1], "velnor-ci plan --full", "{lane}");
        assert_eq!(order[restored + 2], "Format", "{lane}");
    }
    let audit = &text[text.find("  rust-inputs-audit:").ok_or("no audit job")?..];
    let audited = named(audit, "velnor-ci inputs restore");
    assert_eq!(audited.len(), 2, "{audit}");
    assert!(
        audited
            .iter()
            .any(|s| s.contains("working-directory: lab\n"))
    );
    assert!(audited.iter().any(|s| !s.contains("working-directory")));
    Ok(())
}

#[test]
fn a_helper_older_than_0_1_81_gets_no_inputs_restore_and_a_newer_one_gets_it() -> TestResult {
    let old = workflow(&locked("0.1.79")?)?;
    assert!(!old.contains("inputs restore"), "{old}");
    assert!(old.contains("name: velnor-ci restored"), "{old}");
    for version in ["0.1.82", "0.1.100"] {
        let text = workflow(&locked(version)?)?;
        assert_eq!(
            named(&text, "velnor-ci inputs restore").len(),
            1,
            "{version}"
        );
    }
    Ok(())
}

#[test]
fn the_workflow_reads_no_repository_variable_unless_cache_debug_is_opted_in() -> TestResult {
    let text = workflow(&view(&["Cargo.toml"], None)?)?;
    assert!(!text.contains("vars."), "{text}");
    assert!(!text.contains("CARGO_LOG"), "{text}");
    assert_eq!(named(&text, "Upload the cargo fingerprint log").len(), 0);
    let off = workflow(&view(&["Cargo.toml"], Some(b"cache_debug = false\n"))?)?;
    assert_eq!(off, text);
    Ok(())
}

#[test]
fn cargo_logs_fingerprints_only_when_the_repository_variable_is_one() -> TestResult {
    let text = workflow(&view(&["Cargo.toml"], Some(b"cache_debug = true\n"))?)?;
    let cargo: Vec<&str> = steps(&text)
        .into_iter()
        .filter(|step| step.contains(r#""$VELNOR_EXEC" run -- cargo"#))
        .collect();
    assert_ne!(cargo.len(), 0);
    for step in cargo {
        assert!(step.contains(DEBUG_LOG), "{step}");
    }
    let upload = named(&text, "Upload the cargo fingerprint log");
    assert_eq!(upload.len(), 1, "{text}");
    assert!(upload[0].contains("vars.VELNOR_CACHE_DEBUG == ''1''"));
    assert!(upload[0].contains("path: '${{ runner.temp }}/velnor-run.log'"));
    assert!(upload[0].contains("name: 'velnor-cargo-log-${{ github.job }}'"));
    Ok(())
}

fn with_build_script(
    source: &str,
    text: Option<&str>,
) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut repo = view(&["Cargo.toml"], None)?;
    let path = RepoPath::new(source)?;
    for metadata in repo.rust_metadata.values_mut() {
        for member in &mut metadata.members {
            member.targets.push(Target {
                name: TargetName::new("build-script-build")?,
                kinds: vec![TargetKind::BuildScript],
                tests: false,
                doctest: false,
                source: Some(path.clone()),
                requires_features: false,
            });
        }
    }
    if let Some(text) = text {
        repo.files.insert(path, text.as_bytes().to_vec());
    }
    Ok(repo)
}

#[test]
fn a_build_script_without_a_rerun_directive_is_reported_with_the_line_to_add() -> TestResult {
    let silent = with_build_script(
        "build.rs",
        Some("fn main() { println!(\"cargo:rustc-env=A=1\"); }"),
    )?;
    let found = silent_build_scripts(&silent);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].source.as_str(), "build.rs");
    let printed = notes(&silent)?;
    assert_eq!(printed.len(), 1);
    assert!(
        printed[0].contains("build.rs of package package-0"),
        "{printed:?}"
    );
    assert!(printed[0].contains("println!(\"cargo:rerun-if-changed=build.rs\");"));
    for declared in [
        "fn main() { println!(\"cargo:rerun-if-changed=build.rs\"); }",
        "fn main() { println!(\"cargo::rerun-if-changed=build.rs\"); }",
        "fn main() { println!(\"cargo:rerun-if-env-changed=PROTOC\"); }",
        "fn main() { writeln!(out, \"cargo:rerun-if-changed={path}\").ok(); }",
        "fn main() { print!(r#\"cargo::rerun-if-changed=proto\"#); }",
    ] {
        let fine = with_build_script("build.rs", Some(declared))?;
        assert!(silent_build_scripts(&fine).is_empty(), "{declared}");
        assert_eq!(notes(&fine)?.len(), 0);
    }
    Ok(())
}

#[test]
fn a_directive_only_named_in_text_is_not_one_the_build_script_emits() -> TestResult {
    for source in [
        "fn main() { // add cargo:rerun-if-changed=build.rs\n}",
        "/* cargo:rerun-if-changed=build.rs */ fn main() {}",
        "/// cargo:rerun-if-changed=build.rs\nfn main() {}",
        "fn main() { let note = \"cargo:rerun-if-changed=build.rs\"; drop(note); }",
        "fn main() { panic!(\"print cargo:rerun-if-changed=build.rs\"); }",
        "fn main() { eprintln!(\"cargo:rerun-if-env-changed=X\"); }",
        "fn main() { println!(\"rerun-if-changed is not printed\"); }",
        "fn main() { if false { /* cargo:rerun-if-changed=x */ } }",
    ] {
        let repo = with_build_script("build.rs", Some(source))?;
        assert_eq!(silent_build_scripts(&repo).len(), 1, "{source}");
        assert_eq!(notes(&repo)?.len(), 1, "{source}");
    }
    Ok(())
}

#[test]
fn generate_rejects_a_build_script_that_is_a_filesystem_symlink() -> TestResult {
    use crate::release_symlink::{Scratch, git};

    let root = std::env::temp_dir().join(format!(
        "velnor-gen-build-script-link-{}",
        std::process::id()
    ));
    fs::remove_dir_all(&root).ok();
    let scratch = Scratch(root.clone());
    fs::create_dir_all(root.join(".velnor"))?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\nbuild = \"build.rs\"\n",
    )?;
    fs::create_dir_all(root.join("src"))?;
    fs::write(root.join("src/lib.rs"), "")?;
    fs::write(
        root.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
    )?;
    fs::write(root.join(".velnor/rust-ci.toml"), "full_run = \"off\"\n")?;
    fs::write(
        root.join(".velnor/rust-ci.lock"),
        crate::release_fixture::LOCK,
    )?;
    let sample =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/helper-source/view");
    fs::copy(sample.join("mise.toml"), root.join("mise.toml"))?;
    fs::copy(sample.join("mise.lock"), root.join("mise.lock"))?;
    fs::write(root.join("build.rs"), "fn main() {}\n")?;
    git(&root, &["init", "--quiet"])?;
    git(&root, &["add", "--all"])?;

    let cargo = std::path::Path::new(env!("CARGO"));
    let cargo_dir = cargo.parent().ok_or("cargo executable has no parent")?;
    let search_path = std::env::join_paths([
        cargo_dir,
        std::path::Path::new("/usr/bin"),
        std::path::Path::new("/bin"),
    ])?;
    let generate = || {
        Command::new(env!("CARGO_BIN_EXE_velnor-gen"))
            .args(["--root", ".", "generate"])
            .current_dir(&root)
            .env("PATH", &search_path)
            .env("MISE_TRUSTED_CONFIG_PATHS", &root)
            .output()
    };
    let silent = generate()?;
    let silent_stderr = String::from_utf8_lossy(&silent.stderr);
    assert!(silent.status.success(), "{silent_stderr}");
    assert!(
        silent_stderr.contains("build.rs of package fixture"),
        "{silent_stderr}"
    );

    fs::write(
        root.join("build.rs"),
        "fn main() { println!(\"cargo:rerun-if-changed=build.rs\"); }\n",
    )?;
    git(&root, &["add", "build.rs"])?;
    let declared = generate()?;
    let declared_stderr = String::from_utf8_lossy(&declared.stderr);
    assert!(declared.status.success(), "{declared_stderr}");
    assert!(
        !declared_stderr.contains("build.rs of package fixture"),
        "{declared_stderr}"
    );

    fs::remove_file(root.join("build.rs"))?;
    fs::write(root.join("actual.rs"), "fn main() {}\n")?;
    symlink("actual.rs", root.join("build.rs"))?;
    git(&root, &["add", "--all"])?;
    let output = generate()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains("build.rs goes through a symbolic link or is not a regular file"),
        "{stderr}"
    );
    drop(scratch);
    Ok(())
}

#[test]
fn compile_steps_build_the_whole_workspace_because_a_narrower_package_set_unifies_features_differently()
-> TestResult {
    for config in [None, Some(b"shape = \"groups\"\n".as_slice())] {
        let text = workflow(&view(&["Cargo.toml"], config)?)?;
        assert!(!text.contains("PACKAGE_ARGS"), "{text}");
        for run in text
            .lines()
            .filter_map(|line| line.trim().strip_prefix("run: "))
            .map(|run| run.trim_matches('\''))
            .filter(|run| run.contains("\"$VELNOR_EXEC\" run"))
            .filter(|run| {
                run.contains("clippy")
                    || run.contains("nextest")
                    || run.contains("cargo doc")
                    || run.contains("--doc")
            })
        {
            assert!(run.contains("--workspace"), "{run}");
            let arguments = run
                .split_once("\"$VELNOR_EXEC\" ")
                .ok_or("no invocation")?
                .1;
            assert!(!arguments.contains(" -p "), "{run}");
        }
    }
    Ok(())
}

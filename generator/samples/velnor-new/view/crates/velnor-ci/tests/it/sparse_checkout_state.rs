// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

// The sparse-checkout YAML that velnor-gen renders is applied with real git here, and the
// working tree a lane ends up with is compared with the files that lane needs.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::io::Repo;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const HELPER_LOCK: &str =
    include_str!("../../../../generator/samples/helper-lock/view/.velnor/rust-ci.lock");
const WORKFLOW: &str = ".github/workflows/rust-ci.yml";
const CONFIG: &str = "full_run = \"off\"\nsparse_checkout = true\n\n[[inputs]]\nglob = \"data/kept.txt\"\npackages = [\"a\"]\n";
const MISE: &str = include_str!("../../../../generator/samples/feature-lane/view/mise.toml");
const MISE_LOCK: &str = include_str!("../../../../generator/samples/feature-lane/view/mise.lock");

/// Present in every lane, whatever crate it builds.
const ALWAYS: [&str; 5] = [
    ".cargo/config.toml",
    ".velnor/rust-ci.lock",
    ".velnor/rust-ci.toml",
    "mise.lock",
    "mise.toml",
];

fn package(name: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n")
}

fn lock(name: &str) -> String {
    format!("version = 4\n\n[[package]]\nname = \"{name}\"\nversion = \"0.1.0\"\n")
}

/// Two crates and a declared input. Files no lane lists (docs, bench, the other crate's
/// sources, `data/other.txt`, `.gitignore`) must be absent from every lane's checkout.
fn repository() -> Repo {
    let repo = Repo::new("sparse-checkout-state");
    let (package_a, package_b) = (package("a"), package("b"));
    let (lock_a, lock_b) = (lock("a"), lock("b"));
    repo.write_all(&[
        (".velnor/rust-ci.toml", CONFIG),
        (".velnor/rust-ci.lock", HELPER_LOCK),
        (".cargo/config.toml", "[build]\n"),
        ("mise.toml", MISE),
        ("mise.lock", MISE_LOCK),
        ("a/Cargo.toml", package_a.as_str()),
        ("a/Cargo.lock", lock_a.as_str()),
        ("a/src/lib.rs", "pub fn a() {}\n"),
        ("b/Cargo.toml", package_b.as_str()),
        ("b/Cargo.lock", lock_b.as_str()),
        ("b/src/lib.rs", "pub fn b() {}\n"),
        ("data/kept.txt", "kept\n"),
        ("data/other.txt", "other\n"),
        ("docs/notes.md", "notes\n"),
        ("bench/sample.bin", "bytes\n"),
    ]);
    repo.commit();
    repo
}

/// The `sparse-checkout:` block of every job that renders one, with the job id it belongs to.
fn lanes(text: &str) -> Vec<(String, Vec<String>)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut job = String::new();
    let mut found = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if let Some(id) = line
            .strip_prefix("  ")
            .and_then(|rest| rest.strip_suffix(':'))
            .filter(|id| !id.starts_with(' '))
        {
            id.clone_into(&mut job);
        }
        let trimmed = line.trim_start();
        if trimmed != "sparse-checkout: |" {
            continue;
        }
        let indent = line.len() - trimmed.len();
        let patterns = lines[index + 1..]
            .iter()
            .take_while(|next| {
                !next.trim().is_empty() && next.len() - next.trim_start().len() > indent
            })
            .map(|next| next.trim().to_owned())
            .collect();
        found.push((job.clone(), patterns));
    }
    found
}

/// The files each rendered lane must hold, written out by hand from the lane's crate.
fn expected_for(job: &str) -> Option<BTreeSet<String>> {
    let crate_a = ["a/Cargo.lock", "a/Cargo.toml", "a/src/lib.rs"];
    let crate_b = ["b/Cargo.lock", "b/Cargo.toml", "b/src/lib.rs"];
    let listed: Vec<&str> = match job {
        "rust-a" => crate_a.iter().chain(&["b/Cargo.toml"]).copied().collect(),
        "rust-b" => crate_b.iter().chain(&["a/Cargo.toml"]).copied().collect(),
        "rust-inputs-audit" => crate_a.iter().chain(&crate_b).copied().collect(),
        _ => return None,
    };
    Some(
        ALWAYS
            .iter()
            .copied()
            .chain(listed)
            .chain(["data/kept.txt"])
            .map(str::to_owned)
            .collect(),
    )
}

fn git(work: &Path, args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let status = Command::new("git")
        .arg("-C")
        .arg(work)
        .args(args)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("git {args:?} failed").into())
    }
}

/// Clones the repository and applies the lane's patterns the way actions/checkout does for
/// `sparse-checkout-cone-mode: false`.
fn sparse_clone(
    repo: &Repo,
    name: &str,
    patterns: &[String],
) -> Result<Repo, Box<dyn std::error::Error>> {
    let clone = Repo::empty(name);
    let work = clone.root().join("work");
    let status = Command::new("git")
        .args(["clone", "--quiet"])
        .arg(repo.root())
        .arg(&work)
        .status()?;
    assert!(status.success(), "clone");
    let mut args = vec!["sparse-checkout", "set", "--no-cone"];
    args.extend(patterns.iter().map(String::as_str));
    git(&work, &args)?;
    Ok(clone)
}

fn present(work: &Path) -> Result<BTreeSet<String>, Box<dyn std::error::Error>> {
    fn walk(
        root: &Path,
        dir: &Path,
        files: &mut BTreeSet<String>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.file_name().is_some_and(|name| name == ".git") {
                continue;
            }
            if path.is_dir() {
                walk(root, &path, files)?;
            } else {
                let relative = path.strip_prefix(root)?.to_string_lossy().into_owned();
                files.insert(relative);
            }
        }
        Ok(())
    }
    let mut files = BTreeSet::new();
    walk(work, work, &mut files)?;
    Ok(files)
}

fn checked_out(
    repo: &Repo,
    name: &str,
    patterns: &[String],
) -> Result<BTreeSet<String>, Box<dyn std::error::Error>> {
    let clone = sparse_clone(repo, name, patterns)?;
    present(&clone.root().join("work"))
}

#[test]
fn each_lane_checks_out_exactly_the_files_it_needs() -> TestResult {
    let repo = repository();
    let checkout = velnor_gen::io::read_checkout(repo.root())?;
    let text = velnor_gen::generated_files(&checkout.view)?
        .remove(&velnor_model::RepoPath::new(WORKFLOW)?)
        .ok_or("no workflow rendered")?;
    let found = lanes(&text);
    let jobs: Vec<&str> = found.iter().map(|(job, _)| job.as_str()).collect();
    assert_eq!(jobs, ["rust-a", "rust-b", "rust-inputs-audit"], "{text}");
    for (job, patterns) in &found {
        let expected = expected_for(job).ok_or("no expectation for lane")?;
        let tree = checked_out(&repo, &format!("lane-{job}"), patterns)?;
        assert_eq!(tree, expected, "{job}: {patterns:?}");
    }
    Ok(())
}

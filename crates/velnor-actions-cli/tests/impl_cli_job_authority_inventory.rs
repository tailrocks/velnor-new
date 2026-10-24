use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use crate::impl_cli_tmp::{code, init_repo, spawn};

#[test]
fn authority_inventory_cli_is_repeatable() -> Result<(), Box<dyn Error>> {
    let root = crate::impl_cli_tmp::fresh_tempdir("authority-inventory")?;
    init_repo(&root)?;
    add_rust_package(&root)?;

    let preview = root.with_file_name(format!(
        "{}-preview",
        root.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("repo")
    ));
    let preview_text = preview.to_string_lossy().into_owned();
    let generated = spawn(&["generate", "--output-dir", &preview_text], &[], &root)?;
    assert_eq!(
        code(&generated),
        0,
        "fixture generation must pass: {}",
        String::from_utf8_lossy(&generated.stderr)
    );
    append_fixture_roles(&root, &preview.join(".github/workflows"))?;

    let first = spawn(&["authority-inventory"], &[], &root)?;
    let second = spawn(&["authority-inventory"], &[], &root)?;
    assert_eq!(code(&first), 0, "first inventory must pass");
    assert_eq!(code(&second), 0, "second inventory must pass");
    assert_eq!(first.stdout, second.stdout, "inventory bytes must match");
    let inventory: serde_json::Value = serde_json::from_slice(&first.stdout)?;
    let digest = inventory["inventory_sha256"]
        .as_str()
        .ok_or("inventory digest is missing")?;
    assert_eq!(digest.len(), 64);
    crate::impl_cli_tmp::cleanup(&preview);
    crate::impl_cli_tmp::cleanup(&root);
    Ok(())
}

fn add_rust_package(root: &Path) -> Result<(), Box<dyn Error>> {
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"inventory-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::create_dir_all(root.join("src"))?;
    fs::write(root.join("src/lib.rs"), "pub fn fixture() {}\n")?;
    Ok(())
}

fn append_fixture_roles(root: &Path, workflows: &Path) -> Result<(), Box<dyn Error>> {
    let mut paths = Vec::new();
    collect_workflows(workflows, &mut paths)?;
    paths.sort();
    let mut config = fs::read_to_string(root.join(".velnor/config.toml"))?;
    for path in paths {
        let workflow_relative = path
            .strip_prefix(workflows)
            .map_err(|_| "workflow path is outside fixture")?
            .to_string_lossy()
            .replace('\\', "/");
        let relative = format!(".github/workflows/{workflow_relative}");
        let source = fs::read_to_string(path)?;
        for job in workflow_job_keys(&source) {
            write!(
                config,
                "\n[[job_authority]]\nroot_path = \"{relative}\"\nworkflow_path = \"{relative}\"\njob_key = \"{job}\"\nrole = \"verification\"\n"
            )?;
        }
    }
    fs::write(root.join(".velnor/config.toml"), config)?;
    Ok(())
}

fn collect_workflows(directory: &Path, paths: &mut Vec<PathBuf>) -> Result<(), Box<dyn Error>> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_workflows(&path, paths)?;
        } else if path.extension().is_some_and(|extension| {
            extension.eq_ignore_ascii_case("yml") || extension.eq_ignore_ascii_case("yaml")
        }) {
            paths.push(path);
        }
    }
    Ok(())
}

fn workflow_job_keys(source: &str) -> Vec<&str> {
    let mut in_jobs = false;
    let mut jobs = Vec::new();
    for line in source.lines() {
        if !in_jobs {
            in_jobs = line == "jobs:";
            continue;
        }
        if !line.starts_with(char::is_whitespace) {
            break;
        }
        if line.starts_with("  ") && !line.starts_with("   ") && line.ends_with(':') {
            jobs.push(line.trim().trim_end_matches(':'));
        }
    }
    jobs
}

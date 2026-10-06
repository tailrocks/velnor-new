//! Repository-maintenance suites stay required and bind their real source inputs.

use std::error::Error;
use std::path::Path;

#[test]
fn canonical_maintenance_check_is_required_without_adding_product_tasks()
-> Result<(), Box<dyn std::error::Error>> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let preparation = crate::prepare(&root)?;
    let check = preparation
        .config
        .checks
        .iter()
        .find(|check| check.id == "maintenance-helpers")
        .ok_or("missing fixed repository-maintenance check")?;

    assert_eq!(check.task, "test-maintenance-helpers");
    assert_eq!(check.tools, ["cargo-nextest", "rust"]);
    assert_eq!(check.runner.label, "ubuntu-26.04");
    assert!(check.inputs.iter().any(|input| input == "mise.toml"));
    assert!(check.inputs.iter().any(|input| input == "Cargo.lock"));
    assert!(
        preparation
            .workflow
            .ir
            .jobs
            .contains_key("check-maintenance-helpers"),
        "the existing named-check contract emits the helper-suite job"
    );
    assert!(
        preparation.workflow.ir.jobs["required"]
            .needs
            .iter()
            .any(|id| id == "check-maintenance-helpers"),
        "Required gates the repository-maintenance suite"
    );
    assert!(
        preparation
            .discovery
            .proposals
            .iter()
            .any(|task| { task.task_id == "stack/mise/maintenance-helpers/check/default" }),
        "the check remains a named repository-quality obligation"
    );
    for task in &preparation.discovery.proposals {
        assert!(!task.task_id.contains("velnor-actions-freshness"));
        assert!(!task.task_id.contains("velnor-archive-guard"));
    }

    let mise = std::fs::read_to_string(root.join("mise.toml"))?;
    for package in ["velnor-actions-freshness", "velnor-archive-guard"] {
        assert!(
            mise.contains(&format!(
                "nextest run --locked --manifest-path Cargo.toml --package {package} --no-tests fail"
            )),
            "named check must discover and run tests for {package}"
        );
    }
    Ok(())
}

#[test]
fn maintenance_check_declares_archive_and_freshness_policy_inputs() -> Result<(), Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let preparation = crate::prepare(&root)?;
    let check = preparation
        .config
        .checks
        .iter()
        .find(|check| check.id == "maintenance-helpers")
        .ok_or("missing fixed repository-maintenance check")?;
    let declared = check.inputs.iter().map(String::as_str).collect::<Vec<_>>();

    for path in archive_guard_manifest_inputs(&root)? {
        assert!(
            declared.contains(&path.as_str()),
            "archive-guard source manifest input is undeclared: {path}"
        );
    }
    for package in [
        "crates/velnor-actions-freshness",
        "crates/velnor-archive-guard",
    ] {
        for path in files_below(&root, Path::new(package))? {
            assert!(
                declared.contains(&path.as_str()),
                "maintenance package source or test input is undeclared: {path}"
            );
        }
    }
    for path in [
        ".config/nextest.toml",
        ".velnor/version-policy.toml",
        "crates/velnor-actions-freshness/src/inventory.rs",
        "crates/velnor-actions-freshness/tests/freshness_contract.rs",
        "mise.toml",
    ] {
        assert!(
            declared.contains(&path),
            "freshness test or execution input is undeclared: {path}"
        );
    }
    Ok(())
}

fn archive_guard_manifest_inputs(root: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    const MANIFEST: &str = "scripts/archive-guard-inputs.txt";
    let content = std::fs::read_to_string(root.join(MANIFEST))?;
    let mut inputs = vec![MANIFEST.to_owned()];
    for line in content.lines() {
        let (kind, path) = line
            .split_once(' ')
            .ok_or("malformed archive-guard input manifest line")?;
        match kind {
            "file" => inputs.push(path.to_owned()),
            "optional" if root.join(path).exists() => inputs.push(path.to_owned()),
            "optional" => {}
            "tree" => inputs.extend(files_below(root, Path::new(path))?),
            _ => return Err(format!("unknown archive-guard input kind: {kind}").into()),
        }
    }
    inputs.sort();
    inputs.dedup();
    Ok(inputs)
}

fn files_below(root: &Path, directory: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    let mut pending = vec![root.join(directory)];
    let mut files = Vec::new();
    while let Some(current) = pending.pop() {
        for entry in std::fs::read_dir(current)? {
            let entry = entry?;
            let path = entry.path();
            let kind = entry.file_type()?;
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file() {
                let relative = path
                    .strip_prefix(root)?
                    .to_str()
                    .ok_or("non-UTF-8 source path")?;
                files.push(relative.replace(std::path::MAIN_SEPARATOR, "/"));
            } else {
                return Err(format!(
                    "unsupported entry in archive source tree: {}",
                    path.display()
                )
                .into());
            }
        }
    }
    Ok(files)
}

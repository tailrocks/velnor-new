//! T20 tofu preservation: names, events, determinism, validators, atomicity.
//!
//! Every existing pin in the naming/events/determinism/validator/
//! atomicity families stays green and gains tofu coverage: tofu job
//! names in plan/YAML parity, tofu steps through the staged
//! validators, tofu paths in the snapshot brackets.

use std::fs;
use std::path::Path;

use tempfile::TempDir;
use velnor_actions_orchestrator::{finalized_jobs, prepare, render_staged_tree};
use velnor_actions_workflow_jobs::context::{CONCURRENCY_CANCEL, CONCURRENCY_GROUP};
use velnor_actions_workflow_renderer::WORKFLOW_PATH;

use crate::support::{TestResult, fixture_manifest_json, git};

/// Git-initialized pure-tofu repo: `roots` plus `extra` files, no Cargo.
pub(crate) fn pure_tofu_repo(
    roots: &[&str],
    extra: &[(&str, &str)],
) -> Result<TempDir, Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let root = dir.path();
    git(&["init", "-b", "testmain"], root)?;
    git(&["config", "user.email", "test@example.com"], root)?;
    git(&["config", "user.name", "Test"], root)?;
    git(&["config", "commit.gpgsign", "false"], root)?;
    let list = roots
        .iter()
        .map(|root| format!("\"{root}\""))
        .collect::<Vec<_>>()
        .join(", ");
    fs::create_dir_all(root.join(".velnor"))?;
    fs::write(
        root.join(".velnor/config.toml"),
        format!(
            "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[stacks.tofu]\nroots = [{list}]\n"
        ),
    )?;
    fs::write(
        root.join(".velnor/release-manifest.json"),
        fixture_manifest_json(),
    )?;
    for repo_root in roots {
        let target = if *repo_root == "." {
            root.to_path_buf()
        } else {
            root.join(repo_root)
        };
        fs::create_dir_all(&target)?;
        fs::write(target.join("main.tf"), "variable \"x\" {}\n")?;
    }
    for (relative, content) in extra {
        let target = root.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(target, content)?;
    }
    Ok(dir)
}

/// Staged `ci.yml` text `generate` would write for one repo root.
fn staged_yaml(root: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let tree = render_staged_tree(&prepare(root)?)?;
    tree.get(WORKFLOW_PATH)
        .map(str::to_owned)
        .ok_or_else(|| "missing workflow in staged tree".into())
}

/// Header block (`"on":` through `concurrency:`) for cross-fixture compare.
fn header_block(yaml: &str) -> String {
    let mut out = Vec::new();
    let mut in_header = false;
    for line in yaml.lines() {
        if line == "\"on\":" {
            in_header = true;
        }
        if line == "jobs:" {
            break;
        }
        if in_header {
            out.push(line);
        }
    }
    out.join("\n")
}

/// Needs list of one rendered job ID.
fn job_needs(yaml: &str, job: &str) -> Vec<String> {
    let mut needs = Vec::new();
    let mut in_job = false;
    let mut in_needs = false;
    for line in yaml.lines() {
        if line == format!("  {job}:") {
            in_job = true;
            continue;
        }
        if in_job {
            if line.starts_with("  ") && !line.starts_with("   ") && line.ends_with(':') {
                break;
            }
            if line == "    needs:" {
                in_needs = true;
                continue;
            }
            if in_needs {
                if let Some(id) = line.strip_prefix("      - ") {
                    needs.push(id.to_owned());
                } else if line.starts_with("    ") {
                    break;
                }
            }
        }
    }
    needs
}

/// `VELNOR_NEEDS_EXPECTED` inventory parsed from rendered YAML.
fn needs_inventory(yaml: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let line = yaml
        .lines()
        .find(|line| line.contains("VELNOR_NEEDS_EXPECTED:"))
        .ok_or("missing needs-expected line")?;
    let scalar = line
        .split_once("VELNOR_NEEDS_EXPECTED:")
        .ok_or("needs-expected shape")?
        .1
        .trim();
    let unquoted = scalar
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .ok_or("needs-expected quoting")?;
    let mut inventory: Vec<String> = serde_json::from_str(&unquoted.replace("\\\"", "\""))?;
    inventory.sort();
    Ok(inventory)
}

/// Tofu validation roots render `OpenToFu — <root>` check names.
#[test]
fn tofu_root_jobs_carry_opentofu_display_names() -> TestResult {
    let repo = pure_tofu_repo(&["stacks/a", "stacks/b"], &[])?;
    let jobs = finalized_jobs(&prepare(repo.path())?)?;
    for (id, root) in [("tofu-stacks-a", "stacks/a"), ("tofu-stacks-b", "stacks/b")] {
        let job = jobs.get(id).ok_or(format!("missing {id}"))?;
        assert_eq!(
            job.display_name,
            format!("OpenToFu — {root}"),
            "{id} display"
        );
    }
    let yaml = staged_yaml(repo.path())?;
    for root in ["stacks/a", "stacks/b"] {
        assert!(
            yaml.contains(&format!("    name: OpenToFu — {root}")),
            "yaml names {root}:\n{yaml}"
        );
    }
    assert!(
        !yaml.contains("Rust / "),
        "pure-tofu emits no rust display:\n{yaml}"
    );
    Ok(())
}

/// The repository root renders as the dot root, never an empty label.
#[test]
fn repo_root_tofu_job_names_the_dot_root() -> TestResult {
    let repo = pure_tofu_repo(&["."], &[])?;
    let jobs = finalized_jobs(&prepare(repo.path())?)?;
    let (id, job) = jobs
        .iter()
        .find(|(id, _)| id.starts_with("tofu-"))
        .ok_or("missing tofu job")?;
    assert_eq!(job.display_name, "OpenToFu — .", "{id} display");
    let yaml = staged_yaml(repo.path())?;
    assert!(
        yaml.contains("    name: OpenToFu — ."),
        "yaml names the dot root:\n{yaml}"
    );
    Ok(())
}

/// Required needs plan, every tofu job, and lint; gate names stay exact.
#[test]
fn required_needs_span_plan_tofu_and_lint() -> TestResult {
    let repo = pure_tofu_repo(&["stacks/a", "stacks/b"], &[])?;
    let yaml = staged_yaml(repo.path())?;
    let needs = job_needs(&yaml, "required");
    for id in ["plan", "tofu-stacks-a", "tofu-stacks-b", "actionlint"] {
        assert!(
            needs.contains(&id.to_owned()),
            "required needs {id}: {needs:?}"
        );
    }
    assert!(
        yaml.contains("    name: Required"),
        "final gate keeps its check name"
    );
    assert!(
        yaml.contains("    if: always()"),
        "final gate keeps its condition"
    );
    assert!(
        yaml.contains("    name: Actionlint"),
        "lint keeps its check name"
    );
    let inventory = needs_inventory(&yaml)?;
    for id in ["plan", "tofu-stacks-a", "tofu-stacks-b", "actionlint"] {
        assert!(
            inventory.contains(&id.to_owned()),
            "merge inventory carries {id}: {inventory:?}"
        );
    }
    Ok(())
}

/// Tofu renders under the same triggers and concurrency as rust, byte for byte.
#[test]
fn tofu_triggers_and_concurrency_match_rust_byte_for_byte() -> TestResult {
    use velnor_actions_workflow_jobs::context::EXPECTED_PR_TYPES;
    let tofu = pure_tofu_repo(&["stacks/a", "stacks/b"], &[])?;
    let tofu_yaml = staged_yaml(tofu.path())?;
    let rust = crate::support::make_repo(crate::support::config_with_branch())?;
    let rust_yaml = staged_yaml(rust.path())?;
    assert_eq!(
        header_block(&tofu_yaml),
        header_block(&rust_yaml),
        "tofu alters no event"
    );
    for event in EXPECTED_PR_TYPES {
        assert!(
            tofu_yaml.contains(&format!("      - {event}")),
            "pr type {event}:\n{tofu_yaml}"
        );
    }
    assert!(
        tofu_yaml.contains("      - testmain"),
        "one push branch:\n{tofu_yaml}"
    );
    assert!(
        tofu_yaml.contains("  merge_group:"),
        "merge queue stays:\n{tofu_yaml}"
    );
    assert!(
        tofu_yaml.contains(&format!("  group: {CONCURRENCY_GROUP}")),
        "concurrency group exact"
    );
    assert!(
        tofu_yaml.contains(&format!("  cancel-in-progress: {CONCURRENCY_CANCEL}")),
        "cancel-in-progress exact"
    );
    assert!(
        tofu_yaml.contains("  tofu-stacks-a:"),
        "tofu job under the same triggers"
    );
    Ok(())
}

/// Tofu steps render as single-line scalars; Mise provisions tofu, never an action.
#[test]
fn tofu_steps_are_single_line_scalars_without_setup_action() -> TestResult {
    let repo = pure_tofu_repo(&["stacks/a", "stacks/b"], &[])?;
    let yaml = staged_yaml(repo.path())?;
    assert!(
        !yaml.contains("setup-opentofu"),
        "Mise only, no setup action:\n{yaml}"
    );
    let mut chdir_runs = 0;
    for line in yaml.lines() {
        if line.contains("-chdir") {
            assert!(
                line.starts_with("        run: "),
                "single-line scalar: {line}"
            );
            chdir_runs += 1;
        }
    }
    assert!(chdir_runs >= 2, "both roots invoke through -chdir:\n{yaml}");
    assert!(
        yaml.contains("-chdir stacks/a"),
        "root-scoped invocation:\n{yaml}"
    );
    Ok(())
}

/// Tofu display lines carry no expression openers (H3).
#[test]
fn tofu_display_lines_carry_no_expressions() -> TestResult {
    let repo = pure_tofu_repo(&["stacks/a", "stacks/b"], &[])?;
    let yaml = staged_yaml(repo.path())?;
    let mut displays = 0;
    for line in yaml.lines() {
        if line.starts_with("    name: OpenToFu") {
            assert!(!line.contains("${{"), "expression-free: {line}");
            displays += 1;
        }
    }
    assert_eq!(displays, 2, "both tofu displays scanned:\n{yaml}");
    Ok(())
}

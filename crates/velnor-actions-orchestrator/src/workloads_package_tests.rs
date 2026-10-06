//! Selected scripts survive derivation, dependency ordering and rendered execution.

#[path = "../../test_support/git_fixture.rs"]
mod git_fixture;

use std::fs;
use tempfile::TempDir;
use velnor_actions_contract::StepKind;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn fixture(
    kind: &str,
    scripts: &str,
    lock: Option<&str>,
) -> Result<TempDir, Box<dyn std::error::Error>> {
    let root = TempDir::new()?;
    let status = git_fixture::command(root.path())?
        .args(["init", "-b", "testmain"])
        .output()?;
    assert!(status.status.success());
    fs::create_dir_all(root.path().join(".velnor"))?;
    fs::create_dir_all(root.path().join("ui"))?;
    fs::write(root.path().join("ui/package.json"), "{}")?;
    if let Some(lock) = lock {
        fs::write(root.path().join("ui").join(lock), "fixture")?;
    }
    fs::write(
        root.path().join(".velnor/config.toml"),
        format!(
            "schema = 1\n[workflow]\ndefault_branch = \"testmain\"\n[[stacks.workloads]]\nname = \"ui\"\nroot = \"ui\"\nkind = \"{kind}\"\nscripts = [{scripts}]\n"
        ),
    )?;
    Ok(root)
}

#[test]
fn typed_scripts_keep_exact_vectors_phase_dependencies_and_render_order() -> TestResult {
    for (kind, manager, lock, scripts, expected) in [
        (
            "bun_ci",
            "bun",
            "bun.lock",
            "\"typecheck\",\"build\"",
            vec!["install", "typecheck", "build"],
        ),
        (
            "bun_ci",
            "bun",
            "bun.lock",
            "\"lint\",\"typecheck\",\"check\",\"build\",\"test\"",
            vec!["install", "lint", "typecheck", "check", "build", "test"],
        ),
        (
            "node_ci",
            "npm",
            "package-lock.json",
            "\"typecheck\",\"build\"",
            vec!["install", "typecheck", "build"],
        ),
    ] {
        let root = fixture(kind, scripts, Some(lock))?;
        fs::write(
            root.path().join("mise.toml"),
            "[tasks.build]\nrun='untrusted-hook'\n",
        )?;
        let prep = crate::prepare(root.path())?;
        assert_dependencies(&prep, manager, &expected)?;
        assert_commands(&prep, manager, &expected)?;
    }
    Ok(())
}

#[test]
fn node_requires_its_own_lock_not_a_bun_lock() -> TestResult {
    for lock in [None, Some("bun.lock")] {
        let root = fixture("node_ci", "\"build\"", lock)?;
        let error = crate::prepare(root.path()).expect_err("npm ci requires npm lock");
        assert!(
            error.to_string().contains("workload_node_lock_missing:ui"),
            "{error}"
        );
        assert!(!root.path().join(".github").exists());
    }
    Ok(())
}

fn assert_dependencies(
    prep: &crate::GenerationPreparation,
    manager: &str,
    expected: &[&str],
) -> TestResult {
    let tasks = &prep.discovery.proposals;
    assert_eq!(tasks.len(), expected.len());
    for (index, phase) in expected.iter().enumerate() {
        let task = tasks
            .iter()
            .find(|task| task.task_kind == *phase)
            .ok_or("phase missing")?;
        assert!(task.task_id.contains(&format!("/{phase}/")));
        let payload: Vec<_> = task
            .payload
            .iter()
            .map(|arg| arg.to_string_lossy())
            .collect();
        let vector = match (*phase, manager) {
            ("install", "npm") => vec!["npm", "ci"],
            ("install", _) => vec!["bun", "install", "--frozen-lockfile"],
            _ => vec![manager, "run", phase],
        };
        assert_eq!(payload, vector);
        if index == 0 {
            assert!(task.depends_on.is_empty());
        } else {
            let previous = tasks
                .iter()
                .find(|task| task.task_kind == expected[index - 1])
                .ok_or("dependency missing")?;
            assert_eq!(task.depends_on, [previous.task_id.clone()]);
        }
    }
    Ok(())
}

fn assert_commands(
    prep: &crate::GenerationPreparation,
    manager: &str,
    expected: &[&str],
) -> TestResult {
    let (_, job) = prep
        .workflow
        .ir
        .jobs
        .iter()
        .find(|(id, _)| id.starts_with("workload-"))
        .ok_or("job missing")?;
    let commands = job
        .steps
        .iter()
        .filter_map(|step| match &step.kind {
            StepKind::Shell { run, .. } => Some(run.join(" ")),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    let install = if manager == "npm" {
        "npm ci"
    } else {
        "bun install --frozen-lockfile"
    };
    let mut position = commands.find(install).ok_or("install missing")?;
    for phase in &expected[1..] {
        let next = commands
            .find(&format!("{manager} run {phase}"))
            .ok_or("script missing")?;
        assert!(position < next, "{commands}");
        position = next;
    }
    if !expected.contains(&"test") {
        assert!(!commands.contains(&format!("{manager} run test")));
    }
    assert!(!commands.contains("untrusted-hook"));
    assert!(!commands.contains("rust@"));
    assert!(commands.contains("--no-config"));
    assert!(commands.contains("--no-hooks"));
    Ok(())
}

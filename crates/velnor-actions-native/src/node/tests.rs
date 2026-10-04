//! Package command and workspace-lock contracts need no tool execution.

use super::{phases, rank, validate_evidence};
use velnor_actions_contract::config::WorkloadConfig;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn workload(kind: &str, root: &str, scripts: Option<serde_json::Value>) -> WorkloadConfig {
    serde_json::from_value(serde_json::json!({
        "name":"ui", "kind":kind, "root":root, "scripts":scripts,
    }))
    .expect("typed package fixture")
}

#[test]
fn selected_scripts_keep_fixed_manager_vectors_and_shared_order() -> TestResult {
    for (kind, manager, install) in [
        ("bun_ci", "bun", vec!["bun", "install", "--frozen-lockfile"]),
        ("node_ci", "npm", vec!["npm", "ci"]),
    ] {
        let config = workload(kind, "ui", Some(serde_json::json!(["typecheck", "build"])));
        config.validate("config.toml")?;
        let proposed = phases(&config);
        assert_eq!(proposed.len(), 3);
        assert_eq!(
            proposed[0],
            (
                "install",
                install.into_iter().map(str::to_owned).collect::<Vec<_>>()
            )
        );
        assert_eq!(proposed[1].1, [manager, "run", "typecheck"]);
        assert_eq!(proposed[2].1, [manager, "run", "build"]);
        assert!(
            proposed
                .windows(2)
                .all(|pair| rank(pair[0].0) < rank(pair[1].0))
        );
    }
    assert_eq!(
        phases(&workload("bun_ci", ".", None))[2].1,
        ["bun", "run", "test"]
    );
    Ok(())
}

#[test]
fn lock_evidence_binds_selected_manager_and_declared_workspace() -> TestResult {
    let root = tempfile::TempDir::new()?;
    std::fs::create_dir(root.path().join("ui"))?;
    std::fs::write(root.path().join("package-lock.json"), "root lock")?;
    std::fs::write(root.path().join("ui/bun.lock"), "bun lock")?;
    let node = workload("node_ci", "ui", Some(serde_json::json!(["build"])));
    let bun = workload("bun_ci", "ui", None);
    let index = velnor_actions_contract::build_index_walk(root.path(), &[])?;
    assert!(validate_evidence(&bun, &index).is_ok());
    let error = validate_evidence(&node, &index).expect_err("root lock cannot qualify nested npm");
    assert!(error.to_string().contains("workload_node_lock_missing:ui"));
    std::fs::write(root.path().join("ui/package-lock.json"), "workspace lock")?;
    let index = velnor_actions_contract::build_index_walk(root.path(), &[])?;
    assert!(validate_evidence(&node, &index).is_ok());
    Ok(())
}

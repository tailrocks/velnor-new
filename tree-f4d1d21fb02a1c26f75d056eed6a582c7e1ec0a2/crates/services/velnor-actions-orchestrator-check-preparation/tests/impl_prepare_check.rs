//! Check-home preparation refuses unsafe layouts before materializing.

use std::path::Path;
use velnor_actions_mise::{CheckDeadline, DiscoveredCheck};
use velnor_actions_orchestrator_check_preparation::preparation::prepare_check;

fn deadline() -> CheckDeadline {
    CheckDeadline::after(std::time::Duration::from_secs(60)).expect("deadline")
}

fn discovered(root: &Path) -> DiscoveredCheck {
    std::fs::write(root.join("mise.toml"), "[tasks.proof]\nrun = 'true'\n").expect("source");
    let check = serde_json::from_value(serde_json::json!({
        "id":"proof", "task":"proof", "directory":".", "inputs":["mise.toml"], "tools":[],
        "runner":{"label":"ubuntu-24.04", "platform":"linux_x64", "executor":"hosted"}
    }))
    .expect("check");
    velnor_actions_mise::discover_checks(root, &[check], &[])
        .expect("discover")
        .pop()
        .expect("item")
}

#[test]
fn temp_inside_repository_rejected_without_writes() {
    let root = tempfile::TempDir::new().expect("root");
    let item = discovered(root.path());
    let temp = root.path().join("temp");
    std::fs::create_dir(&temp).expect("temp");
    let error = prepare_check(root.path(), &temp, &item, deadline()).expect_err("reject");
    assert!(error.to_string().contains("check_temp_inside_repository"));
    assert_eq!(std::fs::read_dir(temp).expect("entries").count(), 0);
}

#[test]
fn changed_source_refused_before_projection() {
    let root = tempfile::TempDir::new().expect("root");
    let temp = tempfile::TempDir::new().expect("temp");
    let item = discovered(root.path());
    std::fs::write(
        root.path().join("mise.toml"),
        "[tasks.proof]\nrun = 'false'\n",
    )
    .expect("mutate");
    assert!(prepare_check(root.path(), temp.path(), &item, deadline()).is_err());
}

#[test]
fn missing_repository_refused() {
    let root = tempfile::TempDir::new().expect("root");
    let temp = tempfile::TempDir::new().expect("temp");
    let item = discovered(root.path());
    let gone = root.path().join("gone");
    assert!(prepare_check(&gone, temp.path(), &item, deadline()).is_err());
}

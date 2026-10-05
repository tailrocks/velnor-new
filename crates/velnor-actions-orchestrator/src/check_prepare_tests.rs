//! Runtime filesystem ownership preserves source snapshots and refuses escapes.
use super::*;
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
fn source_change_refuses_before_projection_or_execution() {
    let root = tempfile::TempDir::new().expect("root");
    let item = discovered(root.path());
    verify_source(root.path(), &item, deadline()).expect("same bytes");
    std::fs::write(
        root.path().join("mise.toml"),
        "[tasks.proof]\nrun = 'false'\n",
    )
    .expect("mutate");
    assert!(verify_source(root.path(), &item, deadline()).is_err());
}
#[test]
fn owned_homes_cannot_live_inside_repository() {
    let root = tempfile::TempDir::new().expect("root");
    let item = discovered(root.path());
    let temp = root.path().join("temp");
    std::fs::create_dir(&temp).expect("temp");
    let deadline = velnor_actions_mise::CheckDeadline::after(std::time::Duration::from_secs(60))
        .expect("deadline");
    let error = prepare_check(root.path(), &temp, &item, deadline).expect_err("reject");
    assert!(error.to_string().contains("check_temp_inside_repository"));
    assert_eq!(std::fs::read_dir(temp).expect("entries").count(), 0);
}
#[cfg(unix)]
#[test]
fn source_symlink_refuses_even_when_target_stays_in_repository() {
    let root = tempfile::TempDir::new().expect("root");
    let item = discovered(root.path());
    std::fs::rename(
        root.path().join("mise.toml"),
        root.path().join("actual.toml"),
    )
    .expect("move");
    std::os::unix::fs::symlink("actual.toml", root.path().join("mise.toml")).expect("link");
    assert!(verify_source(root.path(), &item, deadline()).is_err());
}

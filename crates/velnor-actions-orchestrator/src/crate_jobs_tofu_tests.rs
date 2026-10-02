//! Tofu crate-job obligation tests (T12).
//!
//! Declared via `#[path]` from `crate_jobs.rs` under `cfg(test)`.

use super::*;
use velnor_actions_rust::TaskKind;

/// Tofu proposal via the T12 adapter constructor.
fn tofu_group(root: &str, kind: velnor_actions_tofu::TofuTaskKind) -> ProposedTask {
    let group = velnor_actions_tofu::TofuTaskGroup {
        root: root.to_owned(),
        kind,
        configuration: "default".to_owned(),
        no_targets: false,
    };
    let task = velnor_actions_tofu::propose_task(&group).expect("fixture proposes");
    task.validate().expect("fixture valid");
    task
}

#[test]
fn tofu_obligations_order_fmt_init_validate() {
    use velnor_actions_tofu::TofuTaskKind;
    let fmt = tofu_group("", TofuTaskKind::Fmt);
    let init = tofu_group("", TofuTaskKind::InitForValidate);
    let validate = tofu_group("", TofuTaskKind::Validate);
    assert!(obligation_rank(&fmt) < obligation_rank(&init));
    assert!(obligation_rank(&init) < obligation_rank(&validate));
    let clippy = crate_jobs_tests::group("demo", TaskKind::Clippy, &[]);
    assert_eq!(obligation_rank(&clippy), task_kind_rank("clippy"));
    let tasks = vec![&validate, &fmt, &init];
    let obligations = obligations_for(&tasks, &ToolCatalog::pinned()).expect("obligations build");
    let kinds: Vec<&str> = obligations
        .iter()
        .map(|obligation| obligation.kind.as_str())
        .collect();
    assert_eq!(kinds, vec!["fmt", "init", "validate"]);
    assert_eq!(obligations[0].step_name, step_name_for("fmt", &fmt.task_id));
    assert_eq!(obligations[2].gated_by, vec![init.task_id.clone()]);
}

//! Job namespaces and plan bindings for explicit workload units.
use super::*;
use velnor_actions_tofu::{TofuTaskGroup, TofuTaskKind};

/// Neutral fixture retains a validated adapter proposal's nonidentity fields.
fn task(stack: Stack, name: &str, configuration: &str) -> ProposedTask {
    let mut task = velnor_actions_tofu::propose_task(&TofuTaskGroup {
        root: name.to_owned(),
        kind: TofuTaskKind::Validate,
        configuration: "default".to_owned(),
        no_targets: false,
    })
    .expect("proposal");
    task.stack_id = stack.id().to_owned();
    task.identity.unit_id = if stack == Stack::Workload {
        format!("workload:{name}")
    } else {
        name.to_owned()
    };
    task.display_name = name.to_owned();
    task.configuration = configuration.to_owned();
    task.validate().expect("valid neutral proposal");
    task
}

#[test]
fn workload_groups_and_plan_bindings_use_workload_namespace() {
    let tasks = vec![
        task(Stack::Workload, "docs", "docs_build"),
        task(Stack::Workload, "docs", "docs_build"),
        task(Stack::Workload, "docs", "ruby_syntax"),
        task(Stack::Rust, "docs", "default"),
        task(Stack::Tofu, "infra", "default"),
    ];
    let grouped = group_runnable(&tasks);
    assert_eq!(grouped.len(), 4);
    let assigned = assign_group_ids(&grouped);
    for (member, expected) in tasks.iter().zip([
        "workload-docs-docs-build",
        "workload-docs-docs-build",
        "workload-docs-ruby-syntax",
        "rust-docs",
        "tofu-infra",
    ]) {
        assert_eq!(job_id_for_member(&tasks, member).as_deref(), Some(expected));
        assert_eq!(
            assigned
                .get(&(
                    member.identity.unit_id.clone(),
                    member.configuration.clone()
                ))
                .map(String::as_str),
            Some(expected)
        );
    }
}

#[test]
fn empty_and_mixed_groups_never_claim_workload_namespace() {
    let workload = task(Stack::Workload, "docs", "docs_build");
    let mut rust = task(Stack::Rust, "docs", "docs_build");
    rust.identity.unit_id.clone_from(&workload.identity.unit_id);
    assert!(!group_is_workload(&[]));
    assert!(!group_is_workload(&[&workload, &rust]));
    let tasks = vec![workload, rust];
    let assigned = assign_group_ids(&group_runnable(&tasks));
    assert_eq!(
        assigned.values().collect::<Vec<_>>(),
        [&"rust-docs-docs-build".to_owned()]
    );
}

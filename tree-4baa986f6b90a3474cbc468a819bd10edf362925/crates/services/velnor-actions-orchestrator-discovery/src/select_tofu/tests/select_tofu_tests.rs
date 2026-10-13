use super::*;

#[test]
fn chdir_findings_name_each_subdir_root_once() {
    use velnor_actions_tofu_core::{TofuTaskGroup, TofuTaskKind};
    let kinds = [
        TofuTaskKind::Fmt,
        TofuTaskKind::InitForValidate,
        TofuTaskKind::Validate,
    ];
    let mut discovery = discovery_with(Vec::new(), Vec::new());
    for root in ["", "stacks/a", "stacks/b"] {
        for kind in kinds {
            let group = TofuTaskGroup {
                root: root.to_owned(),
                kind,
                configuration: "default".to_owned(),
                no_targets: false,
            };
            discovery
                .proposals
                .push(velnor_actions_tofu_core::propose_task(&group).expect("proposes"));
        }
    }
    let mut warnings = Vec::new();
    push_chdir_findings(&discovery, &mut warnings);
    assert_eq!(
        warnings,
        vec![
            "path.cwd:stacks/a".to_owned(),
            "path.cwd:stacks/b".to_owned()
        ]
    );
    let mut root_only = discovery_with(Vec::new(), Vec::new());
    let group = TofuTaskGroup {
        root: String::new(),
        kind: TofuTaskKind::Validate,
        configuration: "default".to_owned(),
        no_targets: false,
    };
    root_only
        .proposals
        .push(velnor_actions_tofu_core::propose_task(&group).expect("proposes"));
    let mut silent = Vec::new();
    push_chdir_findings(&root_only, &mut silent);
    assert!(silent.is_empty());
}

#[test]
fn selected_roots_derive_from_selected_statuses_only() {
    let ignored = DetectionStatus::Ignored {
        project: DetectedProject {
            stack_id: velnor_actions_tofu_core::STACK_ID.to_owned(),
            project_root: "stacks/z".to_owned(),
            manifest: "stacks/z".to_owned(),
        },
        reason: "stack_ignored".to_owned(),
    };
    let roots = tofu_selected_roots(&[selected("stacks/b"), selected(""), ignored]);
    assert_eq!(roots, vec![String::new(), "stacks/b".to_owned()]);
    assert!(tofu_selected_roots(&[]).is_empty());
}

#[test]
fn derive_tofu_proposes_triples_with_fmt_scope_targets() {
    let statuses = vec![selected("stacks/a"), selected("stacks/b")];
    let files = vec![
        "stacks/a/main.tf".to_owned(),
        "stacks/b/main.tf".to_owned(),
        "stacks/b/notes.md".to_owned(),
    ];
    let tasks = derive_tofu(&statuses, &files).expect("derives");
    assert_eq!(tasks.len(), 6);
    for task in &tasks {
        task.validate().expect("valid");
        assert_eq!(task.stack_id, "tofu");
    }
    let fmt: Vec<_> = tasks
        .iter()
        .filter(|task| task.task_kind == "fmt")
        .collect();
    assert_eq!(fmt.len(), 2);
    assert!(fmt.iter().all(|task| !task.no_targets));
    let validate = tasks
        .iter()
        .find(|task| task.task_id == "stack/tofu/stacks/b/validate/default")
        .expect("second validate");
    assert_eq!(
        validate.depends_on,
        vec!["stack/tofu/stacks/b/init/default".to_owned()]
    );
}

#[test]
fn derive_tofu_merges_nested_fmt_scopes() {
    let statuses = vec![selected(""), selected("stacks/a")];
    let files = vec!["main.tf".to_owned(), "stacks/a/main.tf".to_owned()];
    let tasks = derive_tofu(&statuses, &files).expect("derives");
    assert_eq!(tasks.len(), 6);
    let fmt_no_targets = |unit: &str| {
        tasks
            .iter()
            .find(|task| task.task_kind == "fmt" && task.identity.unit_id == unit)
            .map(|task| task.no_targets)
    };
    assert_eq!(fmt_no_targets("root"), Some(false), "outer fmt runs");
    assert_eq!(
        fmt_no_targets("stacks/a"),
        Some(true),
        "covered inner fmt skips"
    );
    assert!(
        tasks
            .iter()
            .filter(|task| task.task_kind != "fmt")
            .all(|task| !task.no_targets),
        "merge never touches init or validate"
    );
}

#[test]
fn derive_tofu_inits_once_per_root() {
    let statuses = vec![
        selected("stacks/a"),
        selected("stacks/b"),
        selected("stacks/a"),
    ];
    let tasks = derive_tofu(&statuses, &[]).expect("derives");
    let mut inits: Vec<&str> = tasks
        .iter()
        .filter(|task| task.task_kind == "init")
        .map(|task| task.task_id.as_str())
        .collect();
    inits.sort_unstable();
    assert_eq!(
        inits,
        [
            "stack/tofu/stacks/a/init/default",
            "stack/tofu/stacks/b/init/default",
        ],
        "duplicate statuses still derive one init per root"
    );
}

#[test]
fn derive_tofu_marks_empty_fmt_scopes_no_targets() {
    let statuses = vec![selected("stacks/empty")];
    let files = vec!["stacks/empty/main.tf.json".to_owned()];
    let tasks = derive_tofu(&statuses, &files).expect("derives");
    let fmt = tasks
        .iter()
        .find(|task| task.task_kind == "fmt")
        .expect("fmt proposes");
    assert!(fmt.no_targets, "JSON-only scope has no fmt targets");
    assert!(
        tasks
            .iter()
            .filter(|task| task.task_kind != "fmt")
            .all(|task| !task.no_targets)
    );
}

#[test]
fn rust_kind_predicates_and_tool_needs_stay_false_for_tofu() {
    use velnor_actions_rust::{is_clippy_kind, is_nextest_kind, is_workspace_fmt_task, tool_needs};
    for kind in ["fmt", "init", "validate"] {
        assert!(!is_clippy_kind(kind), "{kind} never clippy");
        assert!(!is_nextest_kind(kind), "{kind} never nextest");
        assert!(
            !is_workspace_fmt_task(kind, "root", "."),
            "{kind} has a unit"
        );
    }
    let needs = tool_needs("tofu", "none");
    assert!(!needs.mbx && !needs.nextest);
}

#[test]
fn no_tofu_roots_passes_through_without_git() {
    let discovery = discovery_with(Vec::new(), Vec::new());
    let changed: BTreeSet<String> = ["crates/a/src/lib.rs".to_owned()].into_iter().collect();
    let mut warnings = Vec::new();
    let (rust, tofu) = split_and_select(
        Path::new("/nonexistent"),
        "base",
        "head",
        &discovery,
        &changed,
        &mut warnings,
    )
    .expect("passthrough");
    assert_eq!(rust, changed);
    assert!(tofu.is_empty());
    assert!(warnings.is_empty());
}

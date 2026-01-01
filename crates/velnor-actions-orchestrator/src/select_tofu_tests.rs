//! Tofu selection wiring tests.
//!
//! Declared via `#[path]` from `select_tofu.rs` under `cfg(test)`.

use std::collections::BTreeSet;
use std::path::Path;

use super::*;
use velnor_actions_contract::{DetectedProject, DetectionStatus};

/// Selected tofu status for `root`.
fn selected(root: &str) -> DetectionStatus {
    DetectionStatus::Selected(DetectedProject {
        stack_id: velnor_actions_tofu::STACK_ID.to_owned(),
        project_root: root.to_owned(),
        manifest: root.to_owned(),
    })
}

/// Minimal discovery carrying tofu statuses plus selection records.
fn discovery_with(statuses: Vec<DetectionStatus>, units: Vec<TofuSelectionUnit>) -> Discovery {
    Discovery {
        mise_checks: Vec::new(),
        statuses,
        workspaces: Vec::new(),
        proposals: Vec::new(),
        feature_fallbacks: Vec::new(),
        tool_checks: Vec::new(),
        clippy_memory: crate::clippy_groups::ClippyMemoryPlan {
            groups: Vec::new(),
            barriers: 0,
        },
        recommendations: Vec::new(),
        consumer_manifest_json: None,
        consumer_manifest_stand_in: false,
        skipped_non_utf8: false,
        tofu_note: None,
        tofu_units: units,
    }
}

#[test]
fn chdir_findings_name_each_subdir_root_once() {
    use velnor_actions_tofu::{TofuTaskGroup, TofuTaskKind};
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
                .push(velnor_actions_tofu::propose_task(&group).expect("proposes"));
        }
    }
    let mut warnings = Vec::new();
    push_chdir_findings(&discovery, &mut warnings).expect("valid proposal roots");
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
        .push(velnor_actions_tofu::propose_task(&group).expect("proposes"));
    let mut silent = Vec::new();
    push_chdir_findings(&root_only, &mut silent).expect("valid root proposal");
    assert_eq!(silent, Vec::<String>::new());
}

#[test]
fn chdir_findings_reject_a_forged_root_identity() {
    use velnor_actions_tofu::{TofuTaskGroup, TofuTaskKind};
    let mut discovery = discovery_with(Vec::new(), Vec::new());
    let group = TofuTaskGroup {
        root: "stacks/a".to_owned(),
        kind: TofuTaskKind::Validate,
        configuration: "default".to_owned(),
        no_targets: false,
    };
    let mut task = velnor_actions_tofu::propose_task(&group).expect("proposes");
    task.identity.project_root = "stacks/b".to_owned();
    discovery.proposals.push(task);
    assert!(push_chdir_findings(&discovery, &mut Vec::new()).is_err());
}

#[test]
fn selected_roots_derive_from_selected_statuses_only() {
    let ignored = DetectionStatus::Ignored {
        project: DetectedProject {
            stack_id: velnor_actions_tofu::STACK_ID.to_owned(),
            project_root: "stacks/z".to_owned(),
            manifest: "stacks/z".to_owned(),
        },
        reason: "stack_ignored".to_owned(),
    };
    let roots = tofu_selected_roots(&[selected("stacks/b"), selected(""), ignored]);
    assert_eq!(roots, vec![String::new(), "stacks/b".to_owned()]);
    assert_eq!(tofu_selected_roots(&[]), Vec::<String>::new());
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
        .find(|task| task.task_id == "stack/tofu/dir-737461636b732f62/validate/default")
        .expect("second validate");
    assert_eq!(
        validate.depends_on,
        vec!["stack/tofu/dir-737461636b732f62/init/default".to_owned()]
    );
}

#[test]
fn derive_tofu_merges_nested_fmt_scopes() {
    let statuses = vec![selected(""), selected("stacks/a")];
    let files = vec!["main.tf".to_owned(), "stacks/a/main.tf".to_owned()];
    let tasks = derive_tofu(&statuses, &files).expect("derives");
    assert_eq!(tasks.len(), 6);
    let fmt_no_targets = |unit: &str| {
        let key = velnor_actions_tofu::key_for_root(unit);
        tasks
            .iter()
            .find(|task| task.task_kind == "fmt" && task.identity.unit_id == key)
            .map(|task| task.no_targets)
    };
    assert_eq!(fmt_no_targets(""), Some(false), "outer fmt runs");
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
            "stack/tofu/dir-737461636b732f61/init/default",
            "stack/tofu/dir-737461636b732f62/init/default",
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
    assert_eq!(tofu, std::collections::BTreeSet::<String>::new());
    assert_eq!(warnings, Vec::<String>::new());
}

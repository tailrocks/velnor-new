use std::collections::{BTreeMap, BTreeSet};
use velnor_actions_bun::{Member, Workspace, affected_members, forced_members};

use velnor_actions_contract::config::BunInputRule;

fn member(name: &str, root: &str, key: &str, deps: &[&str]) -> Member {
    Member {
        name: name.to_owned(),
        root: root.to_owned(),
        key: key.to_owned(),
        dir: if root.is_empty() {
            key.to_owned()
        } else {
            format!("{root}/{key}")
        },
        scripts: BTreeSet::new(),
        depends_on: deps.iter().map(|dep| (*dep).to_owned()).collect(),
        playwright: None,
    }
}

fn workspace() -> Vec<Workspace> {
    vec![Workspace {
        root: "frontend".to_owned(),
        package_manager: None,
        playwright_core: None,
        members: vec![
            member("contracts", "frontend", "packages/contracts", &[]),
            member("ui", "frontend", "packages/ui", &["contracts"]),
            member("docs", "frontend", "docs", &["ui"]),
            member("scripts", "frontend", "scripts", &[]),
        ],
    }]
}

fn changed(paths: &[&str]) -> BTreeSet<String> {
    paths.iter().map(|path| (*path).to_owned()).collect()
}

fn names(set: &BTreeSet<String>) -> Vec<&str> {
    set.iter().map(String::as_str).collect()
}

#[test]
fn a_member_change_selects_it_and_every_dependent() {
    let selected = affected_members(
        &workspace(),
        &changed(&["frontend/packages/contracts/src/a.ts"]),
        &[],
    );
    assert_eq!(names(&selected), ["contracts", "docs", "ui"]);
    let leaf = affected_members(&workspace(), &changed(&["frontend/scripts/x.ts"]), &[]);
    assert_eq!(names(&leaf), ["scripts"]);
}

#[test]
fn shared_files_select_every_member_of_the_workspace() {
    for path in [
        "frontend/package.json",
        "frontend/bun.lock",
        "frontend/bunfig.toml",
        "frontend/tsconfig.base.json",
        "frontend/patches/a.patch",
    ] {
        let selected = affected_members(&workspace(), &changed(&[path]), &[]);
        assert_eq!(selected.len(), 4, "{path}");
    }
    for path in [
        "frontend/README.md",
        "other/package.json",
        "frontendx/bun.lock",
        "frontend/docs/notes.md",
    ] {
        let selected = affected_members(&workspace(), &changed(&[path]), &[]);
        let expected: &[&str] = if path.starts_with("frontend/docs/") {
            &["docs"]
        } else {
            &[]
        };
        assert_eq!(names(&selected), expected, "{path}");
    }
}

#[test]
fn declared_inputs_select_their_members() {
    let rules = [BunInputRule {
        globs: vec!["frontend/shared/**".to_owned()],
        members: vec!["scripts".to_owned()],
        complete: true,
    }];
    let selected = affected_members(
        &workspace(),
        &changed(&["frontend/shared/a/b.json"]),
        &rules,
    );
    assert_eq!(names(&selected), ["scripts"]);
}

#[test]
fn deepest_member_owns_a_nested_path() {
    let mut spaces = workspace();
    spaces[0]
        .members
        .push(member("nested", "frontend", "docs/nested", &[]));
    let selected = affected_members(&spaces, &changed(&["frontend/docs/nested/a.ts"]), &[]);
    assert_eq!(names(&selected), ["nested"]);
}

#[test]
fn a_root_package_at_the_repository_root_owns_every_path() {
    let spaces = vec![Workspace {
        root: String::new(),
        package_manager: None,
        playwright_core: None,
        members: vec![member("catalog", "", "", &[])],
    }];
    assert_eq!(
        names(&affected_members(&spaces, &changed(&["src/a.ts"]), &[])),
        ["catalog"]
    );
    assert_eq!(
        names(&affected_members(&spaces, &changed(&["bun.lock"]), &[])),
        ["catalog"]
    );
}

#[test]
fn undeclared_outside_reads_force_a_member_until_a_complete_rule_names_it() {
    let reads: BTreeMap<String, Vec<String>> = [
        ("docs".to_owned(), vec!["frontend/packages/x".to_owned()]),
        ("ui".to_owned(), vec!["frontend/shared".to_owned()]),
        ("scripts".to_owned(), Vec::new()),
    ]
    .into();
    let rules = [
        BunInputRule {
            globs: vec!["frontend/shared/**".to_owned()],
            members: vec!["ui".to_owned()],
            complete: true,
        },
        BunInputRule {
            globs: vec!["frontend/packages/x/**".to_owned()],
            members: vec!["docs".to_owned()],
            complete: false,
        },
    ];
    assert_eq!(names(&forced_members(&reads, &rules)), ["docs"]);
    assert_eq!(names(&forced_members(&reads, &[])), ["docs", "ui"]);
}

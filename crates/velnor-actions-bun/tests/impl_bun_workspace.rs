use std::collections::BTreeMap;
use velnor_actions_bun::{job_id, job_name, member_scripts, resolve, slug};

use velnor_actions_contract::config::BunStackConfig;

const ROOT: &str = r#"{"name":"r","private":true,"packageManager":"bun@1.4.2",
  "workspaces":{"packages":["docs","packages/*"]}}"#;
const LOCK: &str = r#"{
  "workspaces": {
    "": {"name": "r"},
    "docs": {"name": "chainargos-docs",
      "dependencies": {"@chainargos/ui": "workspace:*"},
      "devDependencies": {"@playwright/test": "1.63.0"}},
    "packages/ui": {"name": "@chainargos/ui"},
    "packages/contracts": {"name": "@chainargos/contracts"},
  },
  "packages": {"@playwright/test": ["@playwright/test@1.63.0", "", {}, "x"]},
}"#;

fn members() -> BTreeMap<String, String> {
    [
        ("docs", r#"{"name":"chainargos-docs","scripts":{"typecheck":"tsc","test":"vitest","build":"vite build","dev":"vite"}}"#),
        ("packages/ui", r#"{"name":"@chainargos/ui","scripts":{"lint":"oxlint","test":"vitest","check":"x"}}"#),
        ("packages/contracts", r#"{"name":"@chainargos/contracts","scripts":{"format":"oxfmt"}}"#),
    ]
    .into_iter()
    .map(|(key, text)| (key.to_owned(), text.to_owned()))
    .collect()
}

fn config(json: &str) -> Result<BunStackConfig, serde_json::Error> {
    serde_json::from_str(json)
}

#[test]
fn members_are_sorted_with_identity_dependencies_and_playwright() {
    let workspace = resolve("frontend", ROOT, LOCK, &members()).expect("workspace");
    let names: Vec<_> = workspace.members.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        names,
        ["@chainargos/contracts", "@chainargos/ui", "chainargos-docs"]
    );
    let docs = workspace
        .members
        .iter()
        .find(|m| m.name == "chainargos-docs")
        .expect("docs");
    assert_eq!(docs.dir, "frontend/docs");
    assert_eq!(
        docs.depends_on.iter().collect::<Vec<_>>(),
        ["@chainargos/ui"]
    );
    assert_eq!(docs.playwright.as_deref(), Some("1.63.0"));
    assert_eq!(job_id(docs), "bun-chainargos-docs");
    assert_eq!(job_name(docs), "Bun / chainargos-docs");
    assert_eq!(workspace.package_manager.as_deref(), Some("bun@1.4.2"));
}

#[test]
fn single_package_root_is_one_member() {
    let lock = r#"{"workspaces":{"":{"name":"event-catalog"}}}"#;
    let workspace = resolve(
        ".",
        r#"{"name":"event-catalog","scripts":{"build":"x"}}"#,
        lock,
        &BTreeMap::new(),
    )
    .expect("workspace");
    assert_eq!(workspace.members.len(), 1);
    assert_eq!(workspace.members[0].dir, "");
    assert_eq!(job_id(&workspace.members[0]), "bun-event-catalog");
}

#[test]
fn disagreement_between_lock_and_manifests_fails_closed() {
    let mut stale = members();
    stale.remove("docs");
    assert_eq!(
        resolve("f", ROOT, LOCK, &stale)
            .expect_err("missing")
            .code(),
        "bun_member_missing:docs"
    );
    let renamed = LOCK.replace("\"chainargos-docs\"", "\"other\"");
    assert_eq!(
        resolve("f", ROOT, &renamed, &members())
            .expect_err("name")
            .code(),
        "bun_member_name_mismatch:chainargos-docs"
    );
    let undeclared = ROOT.replace("\"docs\",", "");
    assert_eq!(
        resolve("f", &undeclared, LOCK, &members())
            .expect_err("stale")
            .code(),
        "bun_lock_stale"
    );
    let extra = ROOT.replace("\"packages/*\"", "\"packages/*\",\"tools/*\"");
    assert_eq!(
        resolve("f", &extra, LOCK, &members())
            .expect_err("stale")
            .code(),
        "bun_lock_stale"
    );
}

#[test]
fn slugs_never_collide_silently() {
    assert_eq!(slug("@chainargos/ui"), "chainargos-ui");
    assert_eq!(slug("a.b_c"), "a-b-c");
    let lock = r#"{"workspaces":{"":{"name":"r"},"a":{"name":"@x/ui"},"b":{"name":"x-ui"}}}"#;
    let root = r#"{"workspaces":["a","b"]}"#;
    let texts: BTreeMap<String, String> = [
        ("a".to_owned(), r#"{"name":"@x/ui"}"#.to_owned()),
        ("b".to_owned(), r#"{"name":"x-ui"}"#.to_owned()),
    ]
    .into();
    assert_eq!(
        resolve("", root, lock, &texts)
            .expect_err("collision")
            .code(),
        "bun_member_slug_collision:x-ui"
    );
}

#[test]
fn scripts_default_to_the_closed_intersection_and_honour_overrides() {
    let workspace = resolve("frontend", ROOT, LOCK, &members()).expect("workspace");
    let get = |name: &str| {
        workspace
            .members
            .iter()
            .find(|m| m.name == name)
            .expect("member")
            .clone()
    };
    let defaults = config(r#"{"roots":["frontend"]}"#).expect("config");
    assert_eq!(
        member_scripts(&get("chainargos-docs"), &defaults).expect("scripts"),
        ["typecheck", "test", "build"]
    );
    assert_eq!(
        member_scripts(&get("@chainargos/ui"), &defaults).expect("scripts"),
        ["lint", "test"]
    );
    assert_eq!(
        member_scripts(&get("@chainargos/contracts"), &defaults)
            .expect_err("none")
            .code(),
        "bun_member_no_scripts:@chainargos/contracts"
    );
    let custom = config(
        r#"{"roots":["frontend"],"scripts":["test","lint"],"members":{"@chainargos/ui":{"scripts":["check"]}}}"#,
    ).expect("config");
    assert_eq!(
        member_scripts(&get("@chainargos/ui"), &custom).expect("scripts"),
        ["check"]
    );
    assert_eq!(
        member_scripts(&get("chainargos-docs"), &custom).expect("scripts"),
        ["test"]
    );
    let missing =
        config(r#"{"roots":["frontend"],"members":{"@chainargos/ui":{"scripts":["nope"]}}}"#)
            .expect("config");
    assert_eq!(
        member_scripts(&get("@chainargos/ui"), &missing)
            .expect_err("missing")
            .code(),
        "bun_script_missing:@chainargos/ui:nope"
    );
}

#[test]
fn playwright_core_is_the_lock_trust_anchor() {
    let integrity = format!("sha512-{}==", "A".repeat(86));
    let lock = LOCK.replace(
        r#""packages": {"#,
        &format!(r#""packages": {{"playwright-core": ["playwright-core@1.63.0", "", {{}}, "{integrity}"],"#),
    );
    let workspace = resolve("frontend", ROOT, &lock, &members()).expect("workspace");
    let core = workspace.playwright_core.expect("core");
    assert_eq!(core.version, "1.63.0");
    assert_eq!(core.integrity, integrity);
    let bare = resolve("frontend", ROOT, LOCK, &members()).expect("workspace");
    assert!(bare.playwright_core.is_none());
}

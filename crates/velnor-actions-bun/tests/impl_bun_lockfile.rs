use velnor_actions_bun::{check_js_lockfiles, parse_lock};

const LOCK: &str = r#"{
  "lockfileVersion": 2,
  "workspaces": {
    "": { "name": "root", },
    "docs": {
      "name": "chainargos-docs",
      "dependencies": { "@chainargos/ui": "workspace:*", "react": "catalog:", },
      "devDependencies": { "@playwright/test": "1.63.0", },
    },
    "packages/ui": { "name": "@chainargos/ui", },
  },
  "packages": {
    "@playwright/test": ["@playwright/test@1.63.0", "", {}, "sha512-x"],
    "react": ["react@19.2.8", "", {}, "sha512-y"],
  },
}"#;

#[test]
fn only_bun_lock_is_accepted() {
    assert!(check_js_lockfiles(&["package.json", "bun.lock"]).is_ok());
    for foreign in [
        "package-lock.json",
        "npm-shrinkwrap.json",
        "yarn.lock",
        "pnpm-lock.yaml",
        "bun.lockb",
    ] {
        let err = check_js_lockfiles(&["bun.lock", foreign]).expect_err(foreign);
        assert_eq!(err.code(), format!("unsupported_js_lockfile:{foreign}"));
    }
    assert_eq!(
        check_js_lockfiles(&["package.json"])
            .expect_err("none")
            .code(),
        "bun_lock_missing"
    );
}

#[test]
fn workspace_table_and_versions_are_read() {
    let lock = parse_lock(LOCK).expect("lock");
    assert_eq!(lock.workspaces.len(), 3);
    let docs = &lock.workspaces["docs"];
    assert_eq!(docs.name.as_deref(), Some("chainargos-docs"));
    assert_eq!(
        docs.workspace_dependencies()
            .into_iter()
            .collect::<Vec<_>>(),
        ["@chainargos/ui"]
    );
    assert_eq!(lock.versions["@playwright/test"], "1.63.0");
    assert_eq!(lock.versions["react"], "19.2.8");
}

#[test]
fn malformed_locks_fail_closed() {
    for text in ["", "{", "[]", "{\"workspaces\": 3}"] {
        assert_eq!(parse_lock(text).expect_err(text).code(), "bun_lock_invalid");
    }
}

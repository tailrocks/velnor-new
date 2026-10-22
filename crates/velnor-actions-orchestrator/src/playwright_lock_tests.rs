use velnor_actions_contract::config::playwright_lock::{
    LockedArchive, PLAYWRIGHT_HOST_PLATFORM, PLAYWRIGHT_LOCK_SCHEMA,
};

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::config::BunBrowser;

use super::*;

#[test]
fn a_lock_renders_deterministically_and_reads_back_validated() {
    let lock = PlaywrightLock {
        schema: PLAYWRIGHT_LOCK_SCHEMA,
        platform: PLAYWRIGHT_HOST_PLATFORM.to_owned(),
        archives: vec![LockedArchive {
            version: "1.64.1".to_owned(),
            core_integrity: format!("sha512-{}==", "A".repeat(86)),
            browsers: vec!["chromium".to_owned()],
            directory: "chromium-1248".to_owned(),
            url: "https://cdn.playwright.dev/builds/cft/1/linux64/chrome-linux64.zip".to_owned(),
            sha256: "a".repeat(64),
            bytes: 12,
        }],
    };
    let text = render_playwright_lock(&lock).expect("render");
    assert_eq!(text, render_playwright_lock(&lock).expect("again"));
    let dir = tempfile::tempdir().expect("dir");
    assert!(read_playwright_lock(dir.path()).expect("absent").is_none());
    fs::create_dir_all(dir.path().join(".velnor")).expect("velnor");
    fs::write(dir.path().join(PLAYWRIGHT_LOCK_PATH), &text).expect("write");
    assert_eq!(read_playwright_lock(dir.path()).expect("read"), Some(lock));
    fs::write(
        dir.path().join(PLAYWRIGHT_LOCK_PATH),
        text.replace("chromium-1248", "../x-1"),
    )
    .expect("tamper");
    assert!(read_playwright_lock(dir.path()).is_err());
}

#[test]
fn files_are_hashed_in_chunks_with_a_size_bound() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("blob");
    fs::write(&path, b"abc").expect("blob");
    let (sha, bytes) = hash_file(&path, 10).expect("hash");
    assert_eq!(
        sha,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(bytes, 3);
    assert!(hash_file(&path, 2).is_err());
}

fn member(name: &str, playwright: Option<&str>) -> velnor_actions_bun::Member {
    velnor_actions_bun::Member {
        name: name.to_owned(),
        root: "frontend".to_owned(),
        key: name.to_owned(),
        dir: format!("frontend/{name}"),
        scripts: BTreeSet::new(),
        commands: BTreeMap::new(),
        depends_on: BTreeSet::new(),
        playwright: playwright.map(str::to_owned),
    }
}

fn inventory(
    members: Vec<velnor_actions_bun::Member>,
) -> crate::workflow_task_jobs::bun_inventory::BunInventory {
    crate::workflow_task_jobs::bun_inventory::BunInventory {
        roots: vec![crate::workflow_task_jobs::bun_inventory::BunRoot {
            workspace: velnor_actions_bun::Workspace {
                root: "frontend".to_owned(),
                package_manager: None,
                members,
                playwright_core: Some(velnor_actions_bun::PlaywrightCore {
                    version: "1.64.1".to_owned(),
                    integrity: format!("sha512-{}==", "A".repeat(86)),
                }),
            },
            bunfig: None,
        }],
        reads: BTreeMap::new(),
    }
}

fn bun_stack(members: &str) -> velnor_actions_contract::config::BunStackConfig {
    toml::from_str(&format!("roots = [\"frontend\"]\n{members}")).expect("bun stack")
}

#[test]
fn suite_browsers_are_locked_like_member_browsers() {
    // tailrocks/velnor-new#351: a member that declares browsers only on its
    // suites (the documented suite shape) needs the same lock entries.
    let bun = bun_stack(
        "[members.\"site\"]\nbrowsers = [\"chromium\"]\n\
[members.\"site\".suites.browser]\nscripts = [\"test:browser\"]\nbrowsers = [\"chromium\", \"webkit\"]\n\
[members.\"site\".suites.mobile]\nscripts = [\"test:mobile\"]\nbrowsers = [\"firefox\"]\n\
[members.\"docs\".suites.browser]\nscripts = [\"test:browser\"]\nbrowsers = [\"webkit\"]\n\
[members.\"plain\"]\nscripts = [\"test\"]\n",
    );
    let requests = core_requests(
        &bun,
        &inventory(vec![
            member("docs", Some("1.64.1")),
            member("plain", None),
            member("site", Some("1.64.1")),
        ]),
    )
    .expect("requests");
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].version, "1.64.1");
    assert_eq!(
        requests[0].browsers,
        BTreeSet::from([
            BunBrowser::Chromium,
            BunBrowser::Firefox,
            BunBrowser::Webkit
        ])
    );
}

#[test]
fn a_suites_only_member_without_playwright_fails_closed() {
    let bun = bun_stack(
        "[members.\"docs\".suites.browser]\nscripts = [\"test:browser\"]\nbrowsers = [\"webkit\"]\n",
    );
    let error =
        core_requests(&bun, &inventory(vec![member("docs", None)])).expect_err("no playwright");
    assert!(
        error.to_string().contains("bun_playwright_missing:docs"),
        "{error}"
    );
    let none = bun_stack("[members.\"docs\"]\nscripts = [\"test\"]\n");
    assert_eq!(
        core_requests(&none, &inventory(vec![member("docs", Some("1.64.1"))])).expect("empty"),
        Vec::new()
    );
}

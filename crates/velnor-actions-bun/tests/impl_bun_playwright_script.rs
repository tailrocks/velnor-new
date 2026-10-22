//! Which scripts a sharded Playwright suite may append flags to.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_bun::{Member, check_shard_script, parse_package_json};

fn member(scripts: &[(&str, &str)]) -> Member {
    let commands: BTreeMap<String, String> = scripts
        .iter()
        .map(|(name, text)| ((*name).to_owned(), (*text).to_owned()))
        .collect();
    Member {
        name: "@chainargos/platform".to_owned(),
        root: String::new(),
        key: ".".to_owned(),
        dir: String::new(),
        scripts: commands.keys().cloned().collect(),
        commands,
        depends_on: BTreeSet::new(),
        playwright: Some("1.64.1".to_owned()),
    }
}

fn verdict(text: &str) -> Result<(), String> {
    check_shard_script(&member(&[("e2e", text)]), "e2e").map_err(|err| err.code().to_owned())
}

#[test]
fn one_playwright_command_is_admitted_in_every_spelling() {
    for text in [
        "playwright test",
        "playwright test --config=playwright.prod.config.ts",
        "bunx playwright test",
        "bun x playwright test --reporter=dot",
        "bun scripts/with-node.ts playwright test --config playwright.config.ts",
        "  playwright   test\t--grep @smoke ",
        "playwright test --shardx",
    ] {
        assert_eq!(verdict(text), Ok(()), "{text}");
    }
}

#[test]
fn compound_and_shell_syntax_is_rejected() {
    for text in [
        "playwright test && playwright test -c dev.config.ts",
        "playwright test || true",
        "playwright test; echo done",
        "playwright test | tee out.txt",
        "playwright test &",
        "playwright test $(cat args)",
        "playwright test `cat args`",
        "playwright test > out.txt",
        "playwright test\nplaywright test -c dev",
        "playwright test\r",
    ] {
        assert_eq!(
            verdict(text),
            Err("bun_shard_script_compound:e2e".to_owned()),
            "{text:?}"
        );
    }
}

#[test]
fn a_script_that_is_not_playwright_test_is_rejected() {
    for text in [
        "vitest run",
        "playwright show-report",
        "test playwright",
        "playwrighttest",
    ] {
        assert_eq!(
            verdict(text),
            Err("bun_shard_script_not_playwright:e2e".to_owned()),
            "{text}"
        );
    }
}

#[test]
fn scripts_that_choose_shards_projects_or_write_goldens_are_rejected() {
    for text in [
        "playwright test --shard=1/2",
        "playwright test --shard 1/2",
        "playwright test --project=chromium",
        "playwright test --project chromium",
    ] {
        assert_eq!(
            verdict(text),
            Err("bun_shard_script_sets_shard:e2e".to_owned()),
            "{text}"
        );
    }
    for text in [
        "playwright test -u",
        "playwright test --update-snapshots",
        "playwright test --update-snapshots=missing",
    ] {
        assert_eq!(
            verdict(text),
            Err("bun_shard_script_updates_snapshots:e2e".to_owned()),
            "{text}"
        );
    }
}

#[test]
fn a_playwright_script_name_shadows_the_report_merge_and_is_rejected() {
    let shadow = member(&[("e2e", "playwright test"), ("playwright", "echo hijack")]);
    assert_eq!(
        check_shard_script(&shadow, "e2e").map_err(|err| err.code().to_owned()),
        Err("bun_shard_script_name_shadow:@chainargos/platform".to_owned())
    );
    assert_eq!(
        check_shard_script(&member(&[]), "e2e").map_err(|err| err.code().to_owned()),
        Err("bun_script_missing:@chainargos/platform:e2e".to_owned())
    );
}

#[test]
fn package_json_keeps_script_text_for_the_check_only() {
    let package =
        parse_package_json(r#"{"name":"x","scripts":{"test:browser":"playwright test","n":7}}"#)
            .expect("package");
    assert_eq!(
        package.commands.get("test:browser").map(String::as_str),
        Some("playwright test")
    );
    assert!(package.scripts.contains("n"));
    assert!(!package.commands.contains_key("n"));
}

use super::*;
use velnor_ci::io::cache_purge::purge_private_if_public;

fn left(api: &Api) -> Vec<Value> {
    api.entries
        .borrow()
        .iter()
        .map(|e| e["id"].clone())
        .collect()
}

fn others() -> Vec<Value> {
    vec![4, 5, 6, 7, 8].into_iter().map(Value::from).collect()
}

#[test]
fn a_public_repository_loses_its_private_base_entries_and_keeps_the_others() {
    for (private, visibility) in [(false, "public"), (true, "internal")] {
        let api = Api::with_repository(mixed(), private, visibility);
        let report = purge_private_if_public(&api, &repo()).unwrap_or_else(|e| unreachable!("{e}"));
        assert!(report.contains("deleted 3 entries"), "{report}");
        assert_eq!(*api.deleted.borrow(), vec![1, 2, 3]);
        assert_eq!(left(&api), others());
    }
}

#[test]
fn a_private_repository_keeps_its_private_base_entries() {
    let api = Api::with_repository(mixed(), true, "private");
    let report = purge_private_if_public(&api, &repo()).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(report.contains("is private"), "{report}");
    assert!(api.deleted.borrow().is_empty());
    assert_eq!(api.entries.borrow().len(), 8);
}

#[test]
fn a_failed_visibility_lookup_purges_the_private_base_entries() {
    let api = Api::new(mixed());
    let report = purge_private_if_public(&api, &repo()).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(report.contains("deleted 3 entries"), "{report}");
    assert!(report.contains("HTTP 500"), "{report}");
    assert_eq!(*api.deleted.borrow(), vec![1, 2, 3]);
    assert_eq!(left(&api), others());
}

#[test]
fn running_and_enabled_workflows_do_not_stop_the_purge() {
    let api = Api::with_runs(mixed(), &[("in_progress", 2), ("queued", 1)]);
    *api.workflows.borrow_mut() = vec!["active"];
    *api.repository.borrow_mut() =
        Some(json!({"full_name":"tailrocks/lab","private":false,"visibility":"public"}));
    let report = purge_private_if_public(&api, &repo()).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(report.contains("deleted 3 entries"), "{report}");
    assert_eq!(*api.deleted.borrow(), vec![1, 2, 3]);
    assert_eq!(api.run_listings.get(), 0);
}

#[test]
fn a_failed_delete_does_not_stop_the_others_and_every_failure_is_reported() {
    let mut api = Api::with_repository(mixed(), false, "public");
    api.fail_delete = Some(1);
    let error = purge_private_if_public(&api, &repo())
        .err()
        .map(|e| e.to_string());
    assert_eq!(
        error.as_deref(),
        Some("deleted 2 of 3; failed 1 entries: ID 1: HTTP 403")
    );
    assert_eq!(*api.deleted.borrow(), vec![2, 3]);
    assert_eq!(left(&api).len(), 6);
}

#[test]
fn the_flag_selects_the_visibility_gated_purge_and_excludes_yes() {
    use clap::Parser;
    use velnor_ci::cli::{CacheCommand, Cli, Command};
    let parse = |extra: &[&str]| {
        let mut args = vec!["velnor-ci", "cache", "purge-private"];
        args.extend(extra);
        match Cli::try_parse_from(args).map(|cli| cli.command) {
            Ok(Command::Cache(CacheCommand::PurgePrivate(args))) => Some(args.if_public),
            _ => None,
        }
    };
    assert_eq!(parse(&[]), Some(false));
    assert_eq!(parse(&["--if-public"]), Some(true));
    assert_eq!(parse(&["--if-public", "--yes"]), None);
}

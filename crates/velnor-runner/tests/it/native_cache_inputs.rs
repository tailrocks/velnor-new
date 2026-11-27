// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use anyhow::Result;

use crate::exec_support::Scratch;
use crate::native_cache_support::{Message, Store, home_of, run, run_as, run_in, seed};

fn rejected(fixture: &str, edits: &[(&str, &str)], message: &str) -> Result<()> {
    let store = Store::new()?;
    let failed = run(fixture, &store, edits)?;
    assert_eq!(failed.ran.code, Some(1), "{}", failed.ran.stdout);
    assert!(
        failed.ran.stdout.contains(message),
        "{message}\n{}",
        failed.ran.stdout
    );
    assert!(store.entries()?.is_empty());
    Ok(())
}

#[test]
fn a_revision_other_than_the_pinned_one_is_unsupported() -> Result<()> {
    let store = Store::new()?;
    let other = Message {
        revision: Some("v4"),
        ..Message::default()
    };
    let failed = run_as(Scratch::new()?, "cache-restore-exact", &store, &[], &other)?;
    assert_eq!(failed.ran.code, Some(1), "{}", failed.ran.stdout);
    assert!(
        failed
            .ran
            .has_line("unsupported: actions/cache revision 'v4' is not supported by velnor-runner, which implements 55cc8345863c7cc4c66a329aec7e433d2d1c52a9"),
        "{}",
        failed.ran.stdout
    );
    Ok(())
}

fn refused_before_any_step_runs(
    edits: &[(&str, &str)],
    message: &Message<'_>,
    expected: &str,
) -> Result<()> {
    let store = Store::new()?;
    let mut all = vec![("script", "touch ran")];
    all.extend_from_slice(edits);
    let failed = run_as(Scratch::new()?, "cache-save", &store, &all, message)?;
    assert_eq!(failed.ran.code, Some(1), "{}", failed.ran.stdout);
    assert!(failed.ran.has_line(expected), "{}", failed.ran.stdout);
    assert!(
        !failed.workspace_file("ran").exists(),
        "an earlier step ran before the unsupported cache step was refused"
    );
    Ok(())
}

#[test]
fn an_unpinned_revision_is_refused_before_any_earlier_step_runs() -> Result<()> {
    refused_before_any_step_runs(
        &[],
        &Message {
            revision: Some("v4"),
            ..Message::default()
        },
        "unsupported: actions/cache revision 'v4' is not supported by velnor-runner, which implements 55cc8345863c7cc4c66a329aec7e433d2d1c52a9",
    )
}

#[test]
fn an_unsupported_input_is_refused_before_any_earlier_step_runs() -> Result<()> {
    refused_before_any_step_runs(
        &[],
        &Message {
            renamed: &[("key", "fail-on-cache-miss")],
            ..Message::default()
        },
        "unsupported: actions/cache input 'fail-on-cache-miss' is not supported by velnor-runner",
    )
}

#[test]
fn a_literal_glob_path_is_refused_before_any_earlier_step_runs() -> Result<()> {
    refused_before_any_step_runs(
        &[("path", "data/*")],
        &Message::default(),
        "unsupported: actions/cache path 'data/*' uses a glob",
    )
}

#[test]
fn a_comma_in_the_key_is_rejected() -> Result<()> {
    rejected(
        "cache-restore-exact",
        &[("key", "a,b")],
        "Key Validation Error: a,b cannot contain commas.",
    )
}

#[test]
fn a_comma_in_a_restore_key_is_rejected() -> Result<()> {
    rejected(
        "cache-restore-prefix",
        &[("restore-keys", "p-\nx,y")],
        "Key Validation Error: x,y cannot contain commas.",
    )
}

#[test]
fn a_key_over_512_characters_is_rejected() -> Result<()> {
    let key = "k".repeat(513);
    rejected(
        "cache-restore-exact",
        &[("key", &key)],
        "cannot be larger than 512 characters.",
    )
}

#[test]
fn glob_paths_are_unsupported() -> Result<()> {
    for path in ["data/*", "data/a?", "data/[ab]", "!data"] {
        rejected(
            "cache-restore-exact",
            &[("path", path)],
            &format!("unsupported: actions/cache path '{path}' uses a glob"),
        )?;
    }
    Ok(())
}

#[test]
fn ten_keys_in_total_are_accepted_and_eleven_are_rejected() -> Result<()> {
    let store = Store::new()?;
    let nine = (1..=9)
        .map(|n| format!("r{n}-"))
        .collect::<Vec<_>>()
        .join("\n");
    let ok = run("cache-restore-prefix", &store, &[("restore-keys", &nine)])?;
    assert!(ok.succeeded(), "{}", ok.ran.stdout);
    let ten = format!("{nine}\nr10-");
    rejected(
        "cache-restore-prefix",
        &[("restore-keys", &ten)],
        "Key Validation Error: Keys are limited to a maximum of 10.",
    )
}

#[test]
fn saving_a_missing_path_warns_and_succeeds() -> Result<()> {
    let store = Store::new()?;
    let saved = run("cache-save", &store, &[("script", "true")])?;
    assert!(saved.succeeded(), "{}", saved.ran.stdout);
    assert!(
        saved.ran.has_line(
            "[warning]Path Validation Error: Path(s) specified in the action for caching do(es) not exist, hence no cache is being saved."
        ),
        "{}",
        saved.ran.stdout
    );
    assert!(store.entries()?.is_empty());
    Ok(())
}

#[test]
fn a_save_with_an_invalid_key_warns_and_succeeds() -> Result<()> {
    let store = Store::new()?;
    let saved = run("cache-save", &store, &[("key", "a,b")])?;
    assert!(saved.succeeded(), "{}", saved.ran.stdout);
    assert!(
        saved
            .ran
            .has_line("[warning]Key Validation Error: a,b cannot contain commas."),
        "{}",
        saved.ran.stdout
    );
    assert!(store.entries()?.is_empty());
    Ok(())
}

#[test]
fn a_save_without_a_key_warns_and_succeeds() -> Result<()> {
    let store = Store::new()?;
    let saved = run("cache-save", &store, &[("key", "")])?;
    assert!(saved.succeeded(), "{}", saved.ran.stdout);
    assert!(
        saved.ran.has_line("[warning]Key is not specified."),
        "{}",
        saved.ran.stdout
    );
    assert!(store.entries()?.is_empty());
    Ok(())
}

#[test]
fn a_save_without_a_path_warns_and_succeeds() -> Result<()> {
    let store = Store::new()?;
    let saved = run("cache-save", &store, &[("path", "")])?;
    assert!(saved.succeeded(), "{}", saved.ran.stdout);
    assert!(
        saved
            .ran
            .has_line("[warning]Input required and not supplied: path"),
        "{}",
        saved.ran.stdout
    );
    assert!(store.entries()?.is_empty());
    Ok(())
}

#[test]
fn a_restore_without_a_key_fails() -> Result<()> {
    rejected(
        "cache-restore-exact",
        &[("key", "")],
        "Input required and not supplied: key",
    )
}

#[test]
fn a_save_without_a_usable_broker_warns_and_succeeds() -> Result<()> {
    let store = Store::new()?;
    let unavailable = Message {
        github: &[("sha", "bad")],
        ..Message::default()
    };
    let saved = run_as(Scratch::new()?, "cache-save", &store, &[], &unavailable)?;
    assert!(saved.succeeded(), "{}", saved.ran.stdout);
    assert!(
        saved
            .ran
            .has_line("::warning::Failed to save: trusted facts: sha 'bad' is not valid"),
        "{}",
        saved.ran.stdout
    );
    assert!(store.entries()?.is_empty());
    Ok(())
}

#[test]
fn a_tilde_path_expands_to_home() -> Result<()> {
    let store = Store::new()?;
    let script = seed("hello").replace("data", "~/cached");
    let saving = Scratch::new()?;
    let edits = [("path", "~/cached"), ("script", script.as_str())];
    let saved = run_in(saving, "cache-save", &store, &edits)?;
    assert!(saved.succeeded(), "{}", saved.ran.stdout);
    assert!(saved.ran.has_line("Cache saved with key: k-1"));
    let restoring = Scratch::new()?;
    let home = home_of(&restoring);
    let restored = run_in(
        restoring,
        "cache-restore-exact",
        &store,
        &[("path", "~/cached")],
    )?;
    assert!(
        restored
            .ran
            .has_line("hit=[true] matched=[k-1] primary=[k-1]"),
        "{}",
        restored.ran.stdout
    );
    assert_eq!(fs::read_to_string(home.join("cached/a.txt"))?, "hello");
    assert!(!restored.workspace_file("~").exists());
    Ok(())
}

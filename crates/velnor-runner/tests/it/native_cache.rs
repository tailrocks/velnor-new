// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod links;

use std::fs;

use anyhow::Result;
use serde_json::json;

use crate::exec_support::Scratch;
use crate::native_cache_support::{
    Message, Store, run, run_as, saved, seed, seed_and_rewrite_event, trusted_facts,
};

#[test]
fn save_then_restore_roundtrips_files() -> Result<()> {
    let store = Store::new()?;
    saved(&store, "k-1", "hello")?;
    let restored = run("cache-restore-exact", &store, &[])?;
    assert!(restored.succeeded(), "{}", restored.ran.stdout);
    assert_eq!(
        fs::read_to_string(restored.workspace_file("data/a.txt"))?,
        "hello"
    );
    Ok(())
}

#[test]
fn exact_hit_sets_cache_hit_true() -> Result<()> {
    let store = Store::new()?;
    saved(&store, "k-1", "hello")?;
    let restored = run("cache-restore-exact", &store, &[])?;
    assert!(restored.ran.has_line("Cache restored from key: k-1"));
    assert!(
        restored
            .ran
            .has_line("hit=[true] matched=[k-1] primary=[k-1]"),
        "{}",
        restored.ran.stdout
    );
    Ok(())
}

#[test]
fn prefix_hit_sets_cache_hit_false_and_matched_key() -> Result<()> {
    let store = Store::new()?;
    saved(&store, "p-1", "hello")?;
    let restored = run("cache-restore-prefix", &store, &[])?;
    assert!(
        restored
            .ran
            .has_line("hit=[false] matched=[p-1] primary=[p-miss]"),
        "{}",
        restored.ran.stdout
    );
    assert!(restored.workspace_file("data/a.txt").is_file());
    Ok(())
}

#[test]
fn primary_key_prefix_match_sets_cache_hit_false() -> Result<()> {
    let store = Store::new()?;
    saved(&store, "k-1-extra", "hello")?;
    let restored = run("cache-restore-exact", &store, &[])?;
    assert!(
        restored
            .ran
            .has_line("hit=[false] matched=[k-1-extra] primary=[k-1]"),
        "{}",
        restored.ran.stdout
    );
    Ok(())
}

#[test]
fn restore_keys_picks_newest_prefix_match() -> Result<()> {
    let store = Store::new()?;
    for (key, content) in [("p-1", "one"), ("p-2", "two"), ("p-3", "three")] {
        saved(&store, key, content)?;
    }
    store.age("p-1", 300)?;
    store.age("p-3", 200)?;
    store.age("p-2", 100)?;
    let restored = run("cache-restore-prefix", &store, &[])?;
    assert!(
        restored
            .ran
            .has_line("hit=[false] matched=[p-2] primary=[p-miss]"),
        "{}",
        restored.ran.stdout
    );
    assert_eq!(
        fs::read_to_string(restored.workspace_file("data/a.txt"))?,
        "two"
    );
    Ok(())
}

#[test]
fn miss_succeeds_without_hit_outputs() -> Result<()> {
    let store = Store::new()?;
    let restored = run("cache-restore-exact", &store, &[])?;
    assert!(restored.succeeded(), "{}", restored.ran.stdout);
    assert!(restored.ran.has_line("Cache not found for input keys: k-1"));
    assert!(
        restored.ran.has_line("hit=[] matched=[] primary=[k-1]"),
        "{}",
        restored.ran.stdout
    );
    Ok(())
}

#[test]
fn other_path_set_is_a_miss() -> Result<()> {
    let store = Store::new()?;
    saved(&store, "k-1", "hello")?;
    let restored = run("cache-restore-exact", &store, &[("path", "other")])?;
    assert!(restored.succeeded(), "{}", restored.ran.stdout);
    assert!(
        restored.ran.has_line("hit=[] matched=[] primary=[k-1]"),
        "{}",
        restored.ran.stdout
    );
    assert!(!restored.workspace_file("other").exists());
    Ok(())
}

#[test]
fn pull_request_restores_default_branch_entry() -> Result<()> {
    let store = Store::new()?;
    saved(&store, "k-1", "hello")?;
    let restored = run("cache-pull-request", &store, &[])?;
    assert!(restored.succeeded(), "{}", restored.ran.stdout);
    assert!(
        restored
            .ran
            .has_line("hit=[true] matched=[k-1] primary=[k-1]"),
        "{}",
        restored.ran.stdout
    );
    assert!(restored.ran.has_line("hello"));
    Ok(())
}

#[test]
fn pull_request_save_is_not_read_by_default_branch() -> Result<()> {
    let store = Store::new()?;
    let pull = run("cache-pull-request", &store, &[])?;
    assert!(pull.succeeded(), "{}", pull.ran.stdout);
    assert!(pull.ran.has_line("Cache saved with key: pr-1"));
    let main = run("cache-restore-exact", &store, &[("key", "pr-1")])?;
    assert!(
        main.ran.has_line("hit=[] matched=[] primary=[pr-1]"),
        "{}",
        main.ran.stdout
    );
    Ok(())
}

#[test]
fn fork_pull_request_save_writes_nothing() -> Result<()> {
    let store = Store::new()?;
    let fork = run("cache-fork-pull-request", &store, &[])?;
    assert!(fork.succeeded(), "{}", fork.ran.stdout);
    assert!(
        fork.ran
            .has_line("::warning::Failed to save: cache write denied: fork pull request"),
        "{}",
        fork.ran.stdout
    );
    assert!(store.entries()?.is_empty());
    Ok(())
}

#[test]
fn read_mode_save_writes_nothing() -> Result<()> {
    let store = Store::new()?;
    let read = run("cache-read-mode", &store, &[])?;
    assert!(read.succeeded(), "{}", read.ran.stdout);
    assert!(
        read.ran
            .has_line("::warning::Failed to save: cache write denied: cache-mode is read"),
        "{}",
        read.ran.stdout
    );
    assert!(store.entries()?.is_empty());
    Ok(())
}

#[test]
fn lookup_only_restores_nothing() -> Result<()> {
    let store = Store::new()?;
    saved(&store, "k-1", "hello")?;
    let looked = run("cache-lookup-only", &store, &[])?;
    assert!(looked.succeeded(), "{}", looked.ran.stdout);
    assert!(
        looked
            .ran
            .has_line("hit=[true] matched=[k-1] primary=[k-1]"),
        "{}",
        looked.ran.stdout
    );
    assert!(!looked.workspace_file("data").exists());
    Ok(())
}

#[test]
fn second_save_of_same_key_is_noop() -> Result<()> {
    let store = Store::new()?;
    saved(&store, "k-1", "hello")?;
    let script = seed("second");
    let again = run("cache-save", &store, &[("script", &script)])?;
    assert!(again.succeeded(), "{}", again.ran.stdout);
    assert!(again.ran.has_line("Cache already exists for key k-1"));
    assert_eq!(store.entries()?.len(), 1);
    let restored = run("cache-restore-exact", &store, &[])?;
    assert_eq!(
        fs::read_to_string(restored.workspace_file("data/a.txt"))?,
        "hello"
    );
    Ok(())
}

#[test]
fn unsupported_input_fails_with_message() -> Result<()> {
    let store = Store::new()?;
    let failed = run("cache-unsupported-input", &store, &[])?;
    assert!(
        failed.ran.has_line(
            "unsupported: actions/cache input 'fail-on-cache-miss' is not supported by velnor-runner"
        ),
        "{}",
        failed.ran.stdout
    );
    assert_eq!(failed.ran.code, Some(1));
    Ok(())
}

#[test]
fn event_file_rewrite_does_not_change_scope() -> Result<()> {
    let store = Store::new()?;
    let script = seed_and_rewrite_event();
    let pull = run("cache-pull-request", &store, &[("script", &script)])?;
    assert!(pull.succeeded(), "{}", pull.ran.stdout);
    assert!(pull.ran.has_line("Cache saved with key: pr-1"));
    assert_eq!(store.entries()?.len(), 1);
    let seven = run("cache-pull-request", &store, &[("key", "pr-1")])?;
    assert!(
        seven
            .ran
            .has_line("hit=[true] matched=[pr-1] primary=[pr-1]"),
        "{}",
        seven.ran.stdout
    );
    let nine = Message {
        github: &[("ref", "refs/pull/9/merge")],
        ..Message::default()
    };
    let missed = run_as(
        Scratch::new()?,
        "cache-pull-request",
        &store,
        &[("key", "pr-1")],
        &nine,
    )?;
    assert!(
        missed.ran.has_line("hit=[] matched=[] primary=[pr-1]"),
        "{}",
        missed.ran.stdout
    );
    Ok(())
}

#[test]
fn save_scope_comes_from_trusted_facts() -> Result<()> {
    let store = Store::new()?;
    let home = Scratch::new()?;
    let facts = trusted_facts(
        &home,
        &json!({"kind": "pull_request", "number": 8, "base_branch": "main",
            "head": "same_repository"}),
    )?;
    let message = Message {
        trusted: Some(&facts),
        ..Message::default()
    };
    let script = seed("hello");
    for expected in [
        "Cache saved with key: k-1",
        "Cache already exists for key k-1",
    ] {
        let saved = run_as(
            Scratch::new()?,
            "cache-save",
            &store,
            &[("script", &script)],
            &message,
        )?;
        assert!(
            saved.succeeded(),
            "{}\n{}",
            saved.ran.stdout,
            saved.ran.stderr
        );
        assert!(saved.ran.has_line(expected), "{}", saved.ran.stdout);
    }
    assert_eq!(store.entries()?.len(), 1);
    let hit = Message {
        github: &[("ref", "refs/pull/8/merge")],
        ..Message::default()
    };
    let eight = run_as(Scratch::new()?, "cache-pull-request", &store, &[], &hit)?;
    assert!(
        eight.ran.has_line("hit=[true] matched=[k-1] primary=[k-1]"),
        "{}",
        eight.ran.stdout
    );
    for (fixture, message) in [
        ("cache-pull-request", Message::default()),
        ("cache-restore-exact", Message::default()),
    ] {
        let other = run_as(Scratch::new()?, fixture, &store, &[], &message)?;
        assert!(
            other.ran.has_line("hit=[] matched=[] primary=[k-1]"),
            "{}",
            other.ran.stdout
        );
    }
    Ok(())
}

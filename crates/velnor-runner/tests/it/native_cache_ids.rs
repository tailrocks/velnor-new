// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use anyhow::Result;
use serde_json::json;

use crate::exec_support::Scratch;
use crate::native_cache_support::{
    Message, Store, run_as, seed, trusted_facts_of, trusted_facts_on,
};

#[test]
fn an_incoming_path_that_is_a_file_warns_and_publishes_nothing() -> Result<()> {
    let store = Store::new()?;
    fs::create_dir_all(store.root())?;
    fs::write(store.root().join("incoming"), b"not a directory")?;
    let script = seed("hello");
    let saved = run_as(
        Scratch::new()?,
        "cache-save",
        &store,
        &[("script", &script)],
        &Message::default(),
    )?;
    assert!(saved.succeeded(), "{}", saved.ran.stdout);
    assert!(
        saved
            .ran
            .stdout
            .contains("::warning::Failed to save: open the incoming directory"),
        "{}",
        saved.ran.stdout
    );
    assert!(!saved.ran.stdout.contains("Cache saved"));
    assert!(store.entries()?.is_empty());
    Ok(())
}

#[test]
fn a_numeric_repository_id_saves_where_the_string_id_restores() -> Result<()> {
    let store = Store::new()?;
    let script = seed("hello");
    let numeric = Message {
        numbers: &[("repository_id", 4242)],
        ..Message::default()
    };
    let saved = run_as(
        Scratch::new()?,
        "cache-save",
        &store,
        &[("script", &script)],
        &numeric,
    )?;
    assert!(
        saved.ran.has_line("Cache saved with key: k-1"),
        "{}",
        saved.ran.stdout
    );
    let restored = run_as(
        Scratch::new()?,
        "cache-restore-exact",
        &store,
        &[],
        &Message::default(),
    )?;
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
fn the_cache_namespace_comes_only_from_the_broker_facts() -> Result<()> {
    let store = Store::new()?;
    let home = Scratch::new()?;
    let facts = trusted_facts_of(&home, &json!({"kind": "branch", "branch": "main"}), 4242)?;
    let lying = Message {
        github: &[("repository_id", "99")],
        trusted: Some(&facts),
        ..Message::default()
    };
    let script = seed("hello");
    let saved = run_as(
        Scratch::new()?,
        "cache-save",
        &store,
        &[("script", &script)],
        &lying,
    )?;
    assert!(
        saved.ran.has_line("Cache saved with key: k-1"),
        "{}",
        saved.ran.stdout
    );
    let hit = run_as(Scratch::new()?, "cache-restore-exact", &store, &[], &lying)?;
    assert!(
        hit.ran.has_line("hit=[true] matched=[k-1] primary=[k-1]"),
        "{}",
        hit.ran.stdout
    );
    let untrusted = Message {
        github: &[("repository_id", "99")],
        ..Message::default()
    };
    let miss = run_as(
        Scratch::new()?,
        "cache-restore-exact",
        &store,
        &[],
        &untrusted,
    )?;
    assert!(
        miss.ran.has_line("hit=[] matched=[] primary=[k-1]"),
        "{}",
        miss.ran.stdout
    );
    Ok(())
}

#[test]
fn two_github_servers_with_one_repository_id_do_not_share_a_cache() -> Result<()> {
    let store = Store::new()?;
    let event = json!({"kind": "branch", "branch": "main"});
    let home = Scratch::new()?;
    let dotcom = trusted_facts_on(&home, &event, 4242, "https://github.com")?;
    let enterprise = Scratch::new()?;
    let ghes = trusted_facts_on(&enterprise, &event, 4242, "https://ghes.example.com")?;
    let on = |facts| Message {
        trusted: Some(facts),
        ..Message::default()
    };
    let script = seed("hello");
    let saved = run_as(
        Scratch::new()?,
        "cache-save",
        &store,
        &[("script", &script)],
        &on(&dotcom),
    )?;
    assert!(
        saved.ran.has_line("Cache saved with key: k-1"),
        "{}",
        saved.ran.stdout
    );
    let foreign = run_as(
        Scratch::new()?,
        "cache-restore-exact",
        &store,
        &[],
        &on(&ghes),
    )?;
    assert!(
        foreign.ran.has_line("hit=[] matched=[] primary=[k-1]"),
        "{}",
        foreign.ran.stdout
    );
    let own = run_as(
        Scratch::new()?,
        "cache-restore-exact",
        &store,
        &[],
        &on(&dotcom),
    )?;
    assert!(
        own.ran.has_line("hit=[true] matched=[k-1] primary=[k-1]"),
        "{}",
        own.ran.stdout
    );
    Ok(())
}

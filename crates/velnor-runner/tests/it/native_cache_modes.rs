// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;

use crate::exec_support::Scratch;
use crate::native_cache_support::{Message, Store, run, run_as, saved, seed};

const NONE: &[(&str, &str)] = &[("actions_cache_mode", "none")];
const WRITE_ONLY: &[(&str, &str)] = &[("actions_cache_mode", "write-only")];
const UNKNOWN: &[(&str, &str)] = &[("actions_cache_mode", "sometimes")];

fn with_mode(variables: &'static [(&'static str, &'static str)]) -> Message<'static> {
    Message {
        variables,
        ..Message::default()
    }
}

#[test]
fn none_mode_neither_restores_nor_saves() -> Result<()> {
    let store = Store::new()?;
    saved(&store, "k-1", "hello")?;
    let restored = run_as(
        Scratch::new()?,
        "cache-restore-exact",
        &store,
        &[],
        &with_mode(NONE),
    )?;
    assert!(restored.succeeded(), "{}", restored.ran.stdout);
    assert!(
        restored
            .ran
            .has_line("::warning::Failed to restore: cache read denied: cache-mode is none")
    );
    assert!(restored.ran.has_line("hit=[] matched=[] primary=[k-1]"));
    assert!(!restored.workspace_file("data").exists());
    let script = seed("later");
    let saving = run_as(
        Scratch::new()?,
        "cache-save",
        &store,
        &[("key", "k-2"), ("script", &script)],
        &with_mode(NONE),
    )?;
    assert!(saving.succeeded(), "{}", saving.ran.stdout);
    assert!(
        saving
            .ran
            .has_line("::warning::Failed to save: cache write denied: cache-mode is none")
    );
    assert_eq!(store.entries()?.len(), 1);
    Ok(())
}

#[test]
fn write_only_mode_saves_but_never_restores() -> Result<()> {
    let store = Store::new()?;
    saved(&store, "k-1", "hello")?;
    let restored = run_as(
        Scratch::new()?,
        "cache-restore-exact",
        &store,
        &[],
        &with_mode(WRITE_ONLY),
    )?;
    assert!(restored.succeeded(), "{}", restored.ran.stdout);
    assert!(
        restored
            .ran
            .has_line("::warning::Failed to restore: cache read denied: cache-mode is write-only")
    );
    assert!(restored.ran.has_line("hit=[] matched=[] primary=[k-1]"));
    assert!(!restored.workspace_file("data").exists());
    let script = seed("later");
    let saving = run_as(
        Scratch::new()?,
        "cache-save",
        &store,
        &[("key", "k-2"), ("script", &script)],
        &with_mode(WRITE_ONLY),
    )?;
    assert!(saving.ran.has_line("Cache saved with key: k-2"));
    assert_eq!(store.entries()?.len(), 2);
    Ok(())
}

#[test]
fn a_declared_cache_mode_is_not_exported_to_run_steps() -> Result<()> {
    let store = Store::new()?;
    let script = format!(
        "{} && printf 'mode=%s\\n' \"${{ACTIONS_CACHE_MODE-unset}}\"",
        seed("x")
    );
    let saving = run_as(
        Scratch::new()?,
        "cache-save",
        &store,
        &[("script", &script)],
        &with_mode(WRITE_ONLY),
    )?;
    assert!(saving.succeeded(), "{}", saving.ran.stdout);
    assert!(saving.ran.has_line("mode=unset"), "{}", saving.ran.stdout);
    assert!(saving.ran.has_line("Cache saved with key: k-1"));
    Ok(())
}

#[test]
fn a_restore_that_never_reads_the_store_needs_no_broker_facts() -> Result<()> {
    let store = Store::new()?;
    for (mode, name) in [(NONE, "none"), (WRITE_ONLY, "write-only")] {
        let message = Message {
            github: &[("repository_id", "0")],
            variables: mode,
            ..Message::default()
        };
        let restored = run_as(
            Scratch::new()?,
            "cache-restore-exact",
            &store,
            &[],
            &message,
        )?;
        assert!(restored.succeeded(), "{name}: {}", restored.ran.stdout);
        assert!(
            restored.ran.has_line(&format!(
                "::warning::Failed to restore: cache read denied: cache-mode is {name}"
            )),
            "{name}: {}",
            restored.ran.stdout
        );
        assert!(
            restored.ran.has_line("hit=[] matched=[] primary=[k-1]"),
            "{name}: {}",
            restored.ran.stdout
        );
    }
    Ok(())
}

#[test]
fn an_unknown_cache_mode_fails_closed() -> Result<()> {
    let store = Store::new()?;
    let failed = run_as(
        Scratch::new()?,
        "cache-restore-exact",
        &store,
        &[],
        &with_mode(UNKNOWN),
    )?;
    assert_eq!(failed.ran.code, Some(1), "{}", failed.ran.stdout);
    assert!(
        failed
            .ran
            .stdout
            .contains("unsupported: actions/cache mode 'sometimes' is not supported"),
        "{}",
        failed.ran.stdout
    );
    Ok(())
}

#[test]
fn input_names_are_matched_case_insensitively() -> Result<()> {
    let store = Store::new()?;
    let message = Message {
        renamed: &[("key", "KEY"), ("path", "Path")],
        ..Message::default()
    };
    let script = seed("hello");
    let saving = run_as(
        Scratch::new()?,
        "cache-save",
        &store,
        &[("script", &script)],
        &message,
    )?;
    assert!(saving.succeeded(), "{}", saving.ran.stdout);
    assert!(saving.ran.has_line("Cache saved with key: k-1"));
    let restored = run_as(
        Scratch::new()?,
        "cache-restore-exact",
        &store,
        &[],
        &Message {
            renamed: &[("key", "Key"), ("path", "PATH")],
            ..Message::default()
        },
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
fn a_pull_request_event_closed_by_merge_reads_the_branch_scope() -> Result<()> {
    let store = Store::new()?;
    let release = Message {
        github: &[("ref", "refs/heads/release")],
        ..Message::default()
    };
    let script = seed("hello");
    let saving = run_as(
        Scratch::new()?,
        "cache-save",
        &store,
        &[("key", "k-1"), ("script", &script)],
        &release,
    )?;
    assert!(saving.succeeded(), "{}", saving.ran.stdout);
    assert!(
        saving.ran.has_line("Cache saved with key: k-1"),
        "{}",
        saving.ran.stdout
    );
    let merged = Message {
        github: &[
            ("event_name", "pull_request"),
            ("ref", "refs/heads/release"),
            ("base_ref", "release"),
            ("head_ref", "feature"),
        ],
        ..Message::default()
    };
    let restored = run_as(Scratch::new()?, "cache-restore-exact", &store, &[], &merged)?;
    assert!(restored.succeeded(), "{}", restored.ran.stdout);
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
fn a_key_is_measured_in_utf16_units_not_bytes() -> Result<()> {
    let store = Store::new()?;
    let accented = "é".repeat(300);
    let script = seed("hello");
    let saving = run(
        "cache-save",
        &store,
        &[("key", &accented), ("script", &script)],
    )?;
    assert!(saving.succeeded(), "{}", saving.ran.stdout);
    assert!(
        saving
            .ran
            .has_line(&format!("Cache saved with key: {accented}"))
    );
    let restored = run("cache-restore-exact", &store, &[("key", &accented)])?;
    assert!(
        restored.ran.has_line(&format!(
            "hit=[true] matched=[{accented}] primary=[{accented}]"
        )),
        "{}",
        restored.ran.stdout
    );
    let emoji = "😀".repeat(257);
    let failed = run("cache-restore-exact", &store, &[("key", &emoji)])?;
    assert_eq!(failed.ran.code, Some(1), "{}", failed.ran.stdout);
    assert!(
        failed
            .ran
            .stdout
            .contains("cannot be larger than 512 characters.")
    );
    Ok(())
}

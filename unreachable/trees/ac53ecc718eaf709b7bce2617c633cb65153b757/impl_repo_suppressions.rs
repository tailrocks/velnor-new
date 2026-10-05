//! Suppression and lint-set pins: no allows, reasoned expects, exact tables.
//!
//! Split from `impl_repo_strictness` by responsibility: strictness owns
//! validated construction and error taxonomy, this module owns the
//! suppression surface (P11 req 1–2) — every `#[expect]` carries a
//! `reason`, product code holds zero `#[allow]`, and the workspace lint
//! tables contain exactly the qualified individual rules, each deny
//! backed by a compiler rejection fixture.

use std::collections::BTreeSet;
use std::error::Error;

use crate::impl_repo_policy::{p11_toml, read, tree_files};

/// Every deny-level `(table, key)` with its rejection fixture.
const DENY_FIXTURES: [(&str, &str, &str); 20] = [
    ("rust", "unsafe_code", "p11_reject_unsafe.rs"),
    ("rust", "unused_must_use", "p11_reject_must_use.rs"),
    ("rust", "unexpected_cfgs", "p11_reject_unexpected_cfg.rs"),
    (
        "rust",
        "unfulfilled_lint_expectations",
        "p11_reject_unfulfilled_expect.rs",
    ),
    ("clippy", "too_many_lines", "p11_reject_long_fn.rs"),
    ("clippy", "unwrap_used", "p11_reject_unwrap.rs"),
    ("clippy", "expect_used", "p11_reject_expect.rs"),
    ("clippy", "panic", "p11_reject_panic.rs"),
    ("clippy", "todo", "p11_reject_todo.rs"),
    ("clippy", "unimplemented", "p11_reject_unimplemented.rs"),
    ("clippy", "dbg_macro", "p11_reject_dbg.rs"),
    ("clippy", "mem_forget", "p11_reject_mem_forget.rs"),
    ("clippy", "await_holding_lock", "p11_reject_await_lock.rs"),
    (
        "clippy",
        "await_holding_refcell_ref",
        "p11_reject_await_refcell.rs",
    ),
    (
        "clippy",
        "let_underscore_future",
        "p11_reject_underscore_future.rs",
    ),
    (
        "clippy",
        "let_underscore_must_use",
        "p11_reject_underscore_must_use.rs",
    ),
    (
        "clippy",
        "undocumented_unsafe_blocks",
        "p11_reject_undoc_unsafe.rs",
    ),
    (
        "clippy",
        "allow_attributes_without_reason",
        "p11_reject_allow_noreason.rs",
    ),
    (
        "rustdoc",
        "broken_intra_doc_links",
        "p11_reject_broken_link.rs",
    ),
    (
        "rustdoc",
        "private_intra_doc_links",
        "p11_reject_private_link.rs",
    ),
];

/// Effective level of one lint entry: bare word or inline-table `level`.
fn level_of(raw: &str) -> String {
    if raw.trim_start().starts_with('{') {
        for (key, val) in p11_toml::inline_pairs(raw) {
            if key == "level" {
                return val;
            }
        }
        return String::new();
    }
    raw.trim().trim_matches('"').to_owned()
}

#[test]
fn no_allow_suppressions_in_product_code() -> Result<(), Box<dyn Error>> {
    for (dir, _) in crate::impl_repo_policy::MEMBERS {
        for path in tree_files(&format!("{dir}/src"), "rs")? {
            let body = std::fs::read_to_string(&path)?;
            for spelling in ["#[allow", "#![allow"] {
                assert!(
                    !body.contains(spelling),
                    "{} suppresses with {spelling}",
                    path.display()
                );
            }
        }
    }
    Ok(())
}

#[test]
fn every_lint_expect_carries_reason() -> Result<(), Box<dyn Error>> {
    for (dir, _) in crate::impl_repo_policy::MEMBERS {
        for path in tree_files(&format!("{dir}/src"), "rs")? {
            let body = std::fs::read_to_string(&path)?;
            for (index, _) in body.match_indices("#[expect") {
                let rest = &body[index..];
                let end = rest
                    .find(")]")
                    .ok_or_else(|| format!("{} has unterminated expect", path.display()))?;
                assert!(
                    rest[..end].contains("reason"),
                    "{} expects without reason",
                    path.display()
                );
            }
        }
    }
    Ok(())
}

#[test]
fn workspace_lint_tables_have_exact_known_keys() -> Result<(), Box<dyn Error>> {
    let root = p11_toml::parse(&read("Cargo.toml")?)?;
    let expected: [(&str, &[&str]); 3] = [
        (
            "workspace.lints.rust",
            &[
                "unsafe_code",
                "unused_must_use",
                "unexpected_cfgs",
                "unfulfilled_lint_expectations",
                "missing_docs",
                "missing_debug_implementations",
                "unreachable_pub",
                "rust_2018_idioms",
            ],
        ),
        (
            "workspace.lints.clippy",
            &[
                "all",
                "pedantic",
                "too_many_lines",
                "unwrap_used",
                "expect_used",
                "panic",
                "todo",
                "unimplemented",
                "dbg_macro",
                "mem_forget",
                "await_holding_lock",
                "await_holding_refcell_ref",
                "let_underscore_future",
                "let_underscore_must_use",
                "undocumented_unsafe_blocks",
                "allow_attributes_without_reason",
                "allow_attributes",
            ],
        ),
        (
            "workspace.lints.rustdoc",
            &["broken_intra_doc_links", "private_intra_doc_links"],
        ),
    ];
    for (table, want) in expected {
        let section = p11_toml::section(&root, table).ok_or(table)?;
        let mut seen = BTreeSet::new();
        for (key, _) in &section.pairs {
            assert!(!key.contains("restriction"), "{table} enables restriction");
            assert!(!key.contains("nursery"), "{table} enables nursery");
            assert!(seen.insert(key.clone()), "{table} repeats {key}");
        }
        let mut found: Vec<&str> = seen.iter().map(String::as_str).collect();
        found.sort_unstable();
        let mut want: Vec<&str> = want.to_vec();
        want.sort_unstable();
        assert_eq!(found, want, "{table} key drift");
    }
    Ok(())
}

#[test]
fn extra_denies_match_tested_set() -> Result<(), Box<dyn Error>> {
    let root = p11_toml::parse(&read("Cargo.toml")?)?;
    let mut denied = BTreeSet::new();
    for table in ["rust", "clippy", "rustdoc"] {
        let name = format!("workspace.lints.{table}");
        let section = p11_toml::section(&root, &name).ok_or(name)?;
        for (key, raw) in &section.pairs {
            if matches!(level_of(raw).as_str(), "deny" | "forbid") {
                denied.insert((table, key.clone()));
            }
        }
    }
    let mut want = BTreeSet::new();
    for (table, key, fixture) in DENY_FIXTURES {
        want.insert((table, key.to_owned()));
        let body = read(&format!(
            "crates/velnor-actions-cli/tests/fixtures/{fixture}"
        ))?;
        assert!(!body.trim().is_empty(), "{fixture} is empty");
    }
    assert_eq!(denied, want, "deny set drift");
    Ok(())
}

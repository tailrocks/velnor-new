// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use velnor_model::RepoView;

use crate::facts::{TestResult, path};
use crate::pipeline::{pin_helper, view, workflow};
use crate::private_sources::{SECTION, jobs, private_repo};

const GIT_LOCK: &str = "version = 4\n\n[[package]]\nname = \"package-0\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"shared\"\nversion = \"1.0.0\"\nsource = \"git+https://github.com/acme/shared-lib?branch=main#0123456789abcdef\"\n";
const REGISTRY_LOCK: &str = "version = 4\n\n[[package]]\nname = \"shared\"\nversion = \"1.0.0\"\nsource = \"registry+https://registry.example.invalid/index\"\n";
const GROUP: &str = "[[workspace_groups]]\nname = \"tools\"\nroots = [\"tools/a\", \"tools/b\"]\n";
const PRIVATE_SOURCES: &str = "velnor-private-src-";
const PRIVATE_TARGET: &str = "velnor-private-tgt-";

const SHAPES: &[(&str, &[&str], &str)] = &[
    ("lane", &["Cargo.toml"], ""),
    ("lint-test lane", &["Cargo.toml"], "shape = 'lint-test'\n"),
    ("multi-root", &["Cargo.toml", "svc/Cargo.toml"], ""),
    (
        "grouped",
        &["Cargo.toml", "tools/a/Cargo.toml", "tools/b/Cargo.toml"],
        GROUP,
    ),
];

type Rendered = (&'static str, Vec<Value>);

struct Cached {
    key: String,
    restore_keys: Vec<String>,
}

impl Cached {
    fn prefixes(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.key.as_str()).chain(self.restore_keys.iter().map(String::as_str))
    }
}

fn locked(
    roots: &[&str],
    config: &str,
    helper: &str,
    lock: Option<&str>,
) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut repo = private_repo(roots, config, helper)?;
    if let Some(lock) = lock {
        for root in roots {
            let manifest = path(root)?;
            repo.files.insert(
                manifest.sibling(&path("Cargo.lock")?),
                lock.as_bytes().to_vec(),
            );
        }
    }
    Ok(repo)
}

fn render(repo: &RepoView) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    jobs(&workflow(repo)?)
}

fn cached(rendered: &[Value], verb: &str, family: &str) -> Vec<Cached> {
    let wanted = format!("{verb} the {family} cache");
    let mut found = Vec::new();
    for job in rendered {
        for step in job["steps"].as_array().map_or(&[][..], Vec::as_slice) {
            if step["name"] == wanted.as_str() {
                found.push(Cached {
                    key: text(&step["with"]["key"]),
                    restore_keys: text(&step["with"]["restore-keys"])
                        .lines()
                        .map(str::to_owned)
                        .collect(),
                });
            }
        }
    }
    found
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

fn private_lane(
    roots: &[&str],
    extra: &str,
    helper: &str,
) -> Result<RepoView, Box<dyn std::error::Error>> {
    locked(roots, &format!("{extra}{SECTION}"), helper, Some(GIT_LOCK))
}

fn refused_and_public(
    roots: &[&str],
    extra: &str,
    helper: &str,
) -> Result<Vec<(&'static str, RepoView)>, Box<dyn std::error::Error>> {
    let mut plain = view(roots, Some(extra.as_bytes()))?;
    pin_helper(&mut plain, helper)?;
    Ok(vec![
        (
            "refused: private git sources without [private_sources]",
            locked(roots, extra, helper, Some(GIT_LOCK))?,
        ),
        (
            "refused: a registry dependency",
            locked(
                roots,
                &format!("{extra}{SECTION}"),
                helper,
                Some(REGISTRY_LOCK),
            )?,
        ),
        ("public: no private dependency", plain),
        (
            "public: [private_sources] without a private dependency",
            locked(roots, &format!("{extra}{SECTION}"), helper, None)?,
        ),
    ])
}

fn render_all(
    roots: &[&str],
    extra: &str,
    helper: &str,
) -> Result<Vec<Rendered>, Box<dyn std::error::Error>> {
    refused_and_public(roots, extra, helper)?
        .into_iter()
        .map(|(kind, repo)| Ok((kind, render(&repo)?)))
        .collect()
}

fn saves(rendered: &[Value]) -> Vec<Cached> {
    let mut saves = cached(rendered, "Save", "sources");
    saves.extend(cached(rendered, "Save", "target"));
    saves
}

fn restores(rendered: &[Value]) -> Vec<Cached> {
    let mut restores = cached(rendered, "Restore", "sources");
    restores.extend(cached(rendered, "Restore", "target"));
    restores
}

fn private_saves(rendered: &[Value]) -> Vec<Cached> {
    let saves = saves(rendered);
    assert!(
        saves.len() >= 2,
        "the private lane saves no sources and target caches"
    );
    saves
}

fn no_refused_or_public_restore_prefix_matches_a_private_base_save(index: usize) -> TestResult {
    for (shape, roots, extra) in &SHAPES[index..=index] {
        let private = private_saves(&render(&private_lane(roots, extra, "0.1.112")?)?);
        for (kind, rendered) in render_all(roots, extra, "0.1.112")? {
            let restores = restores(&rendered);
            assert!(!restores.is_empty(), "{shape} {kind}: no restores rendered");
            for restore in &restores {
                for prefix in restore.prefixes() {
                    for save in &private {
                        assert!(
                            !save.key.starts_with(prefix),
                            "{shape} {kind}: {prefix} restores the private-base save {}",
                            save.key
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

fn private_base_saves_and_restores_share_the_private_prefix_and_each_other(
    index: usize,
) -> TestResult {
    for (shape, roots, extra) in &SHAPES[index..=index] {
        let rendered = render(&private_lane(roots, extra, "0.1.112")?)?;
        let saved = private_saves(&rendered);
        let restored = restores(&rendered);
        assert!(!restored.is_empty(), "{shape}");
        for each in saved.iter().chain(&restored) {
            assert!(
                each.key.starts_with(PRIVATE_SOURCES) || each.key.starts_with(PRIVATE_TARGET),
                "{shape}: {}",
                each.key
            );
            for prefix in &each.restore_keys {
                assert!(
                    prefix.starts_with(PRIVATE_SOURCES) || prefix.starts_with(PRIVATE_TARGET),
                    "{shape}: {prefix}"
                );
            }
        }
        for restore in &restored {
            assert!(
                saved.iter().any(|save| restore
                    .prefixes()
                    .all(|prefix| save.key.starts_with(prefix))),
                "{shape}: {} has no private save of its own",
                restore.key
            );
        }
    }
    Ok(())
}

fn a_private_lane_at_0_1_111_keeps_the_shared_prefix(index: usize) -> TestResult {
    for (shape, roots, extra) in &SHAPES[index..=index] {
        let repo = private_lane(roots, extra, "0.1.111")?;
        assert!(
            !workflow(&repo)?.contains("velnor-private-"),
            "{shape}: the 0.1.111 render names the private prefix"
        );
        let rendered = render(&repo)?;
        let saved = private_saves(&rendered);
        for each in saved.iter().chain(&restores(&rendered)) {
            assert!(
                each.key.starts_with("velnor-src-") || each.key.starts_with("velnor-tgt-"),
                "{shape}: {}",
                each.key
            );
        }
        let matched = render_all(roots, extra, "0.1.111")?
            .iter()
            .any(|(_, other)| {
                restores(other).iter().any(|restore| {
                    restore
                        .prefixes()
                        .any(|prefix| saved.iter().any(|save| save.key.starts_with(prefix)))
                })
            });
        assert!(
            matched,
            "{shape}: the control lost the overlap it documents"
        );
    }
    Ok(())
}

fn a_refused_or_public_lane_renders_the_same_keys_at_0_1_111_and_0_1_112(
    index: usize,
) -> TestResult {
    for (shape, roots, extra) in &SHAPES[index..=index] {
        let before = render_all(roots, extra, "0.1.111")?;
        let after = render_all(roots, extra, "0.1.112")?;
        for ((kind, old), (_, new)) in before.iter().zip(&after) {
            for (verb, family) in [
                ("Restore", "sources"),
                ("Restore", "target"),
                ("Save", "sources"),
                ("Save", "target"),
            ] {
                let old = cached(old, verb, family);
                let new = cached(new, verb, family);
                assert_eq!(old.len(), new.len(), "{shape} {kind} {verb} {family}");
                for (old, new) in old.iter().zip(&new) {
                    assert_eq!(old.key, new.key, "{shape} {kind}");
                    assert_eq!(old.restore_keys, new.restore_keys, "{shape} {kind}");
                }
            }
        }
    }
    Ok(())
}

macro_rules! per_shape {
    ($($check:ident),*) => {
        $(
            mod $check {
                #[test]
                fn lane() -> crate::facts::TestResult {
                    super::$check(0)
                }

                #[test]
                fn lint_test_lane() -> crate::facts::TestResult {
                    super::$check(1)
                }

                #[test]
                fn multi_root() -> crate::facts::TestResult {
                    super::$check(2)
                }

                #[test]
                fn grouped() -> crate::facts::TestResult {
                    super::$check(3)
                }
            }
        )*
    };
}

per_shape!(
    no_refused_or_public_restore_prefix_matches_a_private_base_save,
    private_base_saves_and_restores_share_the_private_prefix_and_each_other,
    a_private_lane_at_0_1_111_keeps_the_shared_prefix,
    a_refused_or_public_lane_renders_the_same_keys_at_0_1_111_and_0_1_112
);

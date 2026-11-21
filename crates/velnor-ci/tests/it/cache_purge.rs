// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;

use velnor_ci::cache::purge_private;
use velnor_ci::error::CiError;
use velnor_ci::github::{GitHub, GitHubWrite, Repo};

const PRIVATE: &str = r#"{"full_name":"acme/app","private":true,"visibility":"private"}"#;
const PUBLIC: &str = r#"{"full_name":"acme/app","private":false,"visibility":"public"}"#;
const INTERNAL: &str = r#"{"full_name":"acme/app","private":true,"visibility":"internal"}"#;

const ENTRIES: [(u64, &str); 6] = [
    (1, "velnor-private-src-Linux-X64-all.roots-aaaa"),
    (2, "velnor-private-tgt-rust-lab.Linux-X64-x86-abcd-sha-1"),
    (3, "velnor-src-Linux-X64-all.roots-bbbb"),
    (4, "velnor-tgt-rust-lab.Linux-X64-x86-abcd-sha-2"),
    (5, "velnor-tools-Linux-X64-mise2026-cccc"),
    (6, "velnor-dlt-pr7-rust-lab.Linux-X64-x86-abcd-9-1"),
];

struct Fake {
    repository: Option<&'static str>,
    deleted: RefCell<Vec<String>>,
}

impl Fake {
    fn new(repository: Option<&'static str>) -> Self {
        Self {
            repository,
            deleted: RefCell::new(Vec::new()),
        }
    }

    fn listing() -> String {
        let caches: Vec<String> = ENTRIES
            .iter()
            .map(|(id, key)| {
                format!(
                    r#"{{"id":{id},"key":"{key}","ref":"refs/heads/main","size_in_bytes":1,"created_at":"2026-11-01T10:00:00Z","last_accessed_at":"2026-11-01T10:00:00Z"}}"#
                )
            })
            .collect();
        format!(
            r#"{{"total_count":{},"actions_caches":[{}]}}"#,
            ENTRIES.len(),
            caches.join(",")
        )
    }
}

impl GitHub for Fake {
    fn get(&self, path: &str) -> Result<String, CiError> {
        if path == "/repos/acme/app" {
            return self
                .repository
                .map(str::to_owned)
                .ok_or_else(|| CiError::new("GET /repos/acme/app failed"));
        }
        if path.starts_with("/repos/acme/app/actions/caches?") {
            return Ok(Self::listing());
        }
        Err(CiError::new(format!("unexpected request {path}")))
    }
}

impl GitHubWrite for Fake {
    fn wait(&self, _: std::time::Duration) {}

    fn upload(&self, _: &str, _: &[u8]) -> Result<String, CiError> {
        Err(CiError::new("unused"))
    }
    fn patch(&self, _: &str, _: &str) -> Result<String, CiError> {
        Err(CiError::new("unused"))
    }
    fn delete(&self, path: &str) -> Result<(), CiError> {
        self.deleted.borrow_mut().push(path.to_owned());
        Ok(())
    }
}

fn purge(repository: Option<&'static str>) -> Result<(Fake, String), CiError> {
    let fake = Fake::new(repository);
    let report = purge_private(&fake, &Repo::parse("acme/app")?)?;
    Ok((fake, report))
}

#[test]
fn a_public_repository_loses_its_private_base_entries_and_keeps_the_others() -> Result<(), CiError>
{
    for repository in [Some(PUBLIC), Some(INTERNAL)] {
        let (fake, report) = purge(repository)?;
        assert_eq!(
            fake.deleted.borrow().as_slice(),
            [
                "/repos/acme/app/actions/caches/1",
                "/repos/acme/app/actions/caches/2"
            ],
            "{report}"
        );
        assert!(report.contains("deleted 2"), "{report}");
    }
    Ok(())
}

#[test]
fn a_private_repository_keeps_its_private_base_entries() -> Result<(), CiError> {
    let (fake, report) = purge(Some(PRIVATE))?;
    assert!(fake.deleted.borrow().is_empty(), "{report}");
    assert!(report.contains("private"), "{report}");
    Ok(())
}

#[test]
fn a_failed_visibility_lookup_purges_the_private_base_entries() -> Result<(), CiError> {
    let (fake, report) = purge(None)?;
    assert_eq!(
        fake.deleted.borrow().as_slice(),
        [
            "/repos/acme/app/actions/caches/1",
            "/repos/acme/app/actions/caches/2"
        ],
        "{report}"
    );
    Ok(())
}

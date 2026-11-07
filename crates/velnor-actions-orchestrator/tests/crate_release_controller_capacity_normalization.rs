// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    overflow::{PublishedIndex, PublishedRegistry},
    *,
};

#[test]
fn cargo_trimmed_dev_features_do_not_block_publication_preflight()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (mut intent, mut request, mut github, registry) = make_fixture(root)?;
    let index = PublishedIndex::start()?;
    let cargo_home = tempfile::tempdir()?;
    let sha = prepare_workspace(root, cargo_home.path(), &index)?;
    intent.recovery.as_mut().ok_or("recovery")?.source_sha = sha.clone();
    request.source_sha = sha.clone();
    request.recovery_sha = Some(sha.clone());
    github.sha = sha;
    github.approved.set(true);
    assert_normalized_archive(root, cargo_home.path())?;
    super::FIXTURE_SPARSE_INDEX.with(|value| *value.borrow_mut() = Some(index.url.clone()));
    let registry = PublishedRegistry(registry);
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let (identity, receipt) = fixture_journal_plan(root, &intent, &request, &admission)?;
    let journal = JournalRegistryFake::default();
    seed_started_record(&journal, &identity, &receipt)?;
    journal.write_calls.set(0);
    let input = CrateReleaseObservation {
        source_execution: &crate::UnprivilegedSourceExecution,
        git: test_git(),
        journal: &journal,
        identity: &identity,
        journal_receipt: &receipt,
        admission: &admission,
        intent: &intent,
        request: &request,
        github: &github,
        registry: &registry,
        source: root,
        sequence: 1,
    };
    let calls = Cell::new(0);
    let result = coordinate_release(
        &input,
        |_| {
            calls.set(calls.get() + 1);
            Err(crate::internal::internal("capacity_fixture_stop"))
        },
        |_| panic!("unexpected upload"),
        || {},
    );
    assert!(result.is_err());
    assert_eq!(calls.get(), 1);
    assert_eq!(journal.write_calls.get(), 0);
    assert_eq!(github.writes.get(), 0);
    super::FIXTURE_SPARSE_INDEX.with(|value| *value.borrow_mut() = None);
    Ok(())
}

fn prepare_workspace(
    root: &Path,
    cargo_home: &Path,
    index: &PublishedIndex,
) -> Result<String, Box<dyn std::error::Error>> {
    fs::write(
        cargo_home.join("config.toml"),
        format!(
            "[source.crates-io]\nreplace-with='fixture'\n[source.fixture]\nregistry='sparse+{}/'\n",
            index.url
        ),
    )?;
    for name in ["ghost_direct", "ghost_inherit", "ghost_target"] {
        fs::create_dir_all(root.join(name).join("src"))?;
        fs::write(root.join(name).join("src/lib.rs"), "pub fn f() {}")?;
        fs::write(
            root.join(name).join("Cargo.toml"),
            format!(
                "[package]\nname='{name}'\nversion='0.1.0'\nedition='2021'\npublish=false\n[features]\nfoo=[]\n"
            ),
        )?;
    }
    let members = (0..10_002)
        .map(|index| match index % 3 {
            0 => "'ghost_direct/foo'",
            1 => "'ghost_inherit/foo'",
            _ => "'ghost_target/foo'",
        })
        .collect::<Vec<_>>()
        .join(",");
    let manifest = format!(
        "[package]\nname='demo'\nversion='0.1.1'\nedition='2021'\ndescription='fixture'\nlicense='MIT'\n[features]\ntrim=[{members}]\nretained=['percent-encoding/alloc']\n[workspace]\nmembers=['ghost_direct','ghost_inherit','ghost_target']\nresolver='2'\n[workspace.dependencies]\nghost_inherit={{path='ghost_inherit'}}\n[dev-dependencies]\nghost_direct={{path='ghost_direct'}}\nghost_inherit={{workspace=true}}\npercent-encoding='=2.3.2'\n[target.'cfg(unix)'.dev-dependencies]\nghost_target={{path='ghost_target'}}\n"
    );
    fs::write(root.join("Cargo.toml"), manifest)?;
    let output = Command::new(env!("CARGO"))
        .args(["generate-lockfile"])
        .current_dir(root)
        .env("CARGO_HOME", cargo_home)
        .env_remove("CARGO_NET_OFFLINE")
        .output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    commit_paths(root)
}

fn assert_normalized_archive(
    root: &Path,
    cargo_home: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let target = tempfile::tempdir()?;
    let output = Command::new(env!("CARGO"))
        .args([
            "package",
            "--locked",
            "--offline",
            "--no-verify",
            "--target-dir",
        ])
        .arg(target.path())
        .current_dir(root)
        .env("CARGO_HOME", cargo_home)
        .output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    let archive = fs::read(target.path().join("package/demo-0.1.1.crate"))?;
    let archived =
        crate::release::crate_release_archive::crate_archive_features(&archive, "demo", "0.1.1")?;
    assert_eq!(archived.get("trim"), Some(&Vec::new()));
    assert_eq!(
        archived.get("retained"),
        Some(&vec!["percent-encoding/alloc".to_owned()])
    );
    Ok(())
}

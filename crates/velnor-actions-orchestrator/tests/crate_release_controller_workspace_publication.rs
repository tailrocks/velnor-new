// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[test]
fn coordinator_publishes_two_new_workspace_crates_only_with_recorded_owners()
-> Result<(), Box<dyn std::error::Error>> {
    run_two_new_workspace_crates(false)?;
    run_two_new_workspace_crates(true)
}

fn run_two_new_workspace_crates(change_owner: bool) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::CrateReleaseObservation;
    use crate::crate_release_journal::{
        JournalAuthentication, JournalOwnership, record_source_bound_start,
    };
    let (temp, mut intent, mut request, original_github, first) = make_workspace_order_fixture()?;
    let root = temp.path();
    intent.authentication = velnor_actions_contract::config::ReleaseAuthentication::BootstrapToken;
    intent.new_crates = Some(velnor_actions_contract::config::NewCrateRelease {
        source_sha: request.source_sha.clone(),
        packages: intent
            .packages
            .iter()
            .map(|name| (name.clone(), "0.1.1".into()))
            .collect(),
    });
    request.event = "workflow_dispatch".into();
    request.recovery_sha = Some(request.source_sha.clone());
    let github = CreationWorkspaceGithub(MixedGithub {
        inner: original_github,
        tags: [Cell::new(false), Cell::new(false)],
        releases: [Cell::new(false), Cell::new(false)],
        writes: RefCell::new(Vec::new()),
    });
    let registry = CreationWorkspaceRegistry {
        inner: MixedRegistry {
            first,
            second: RefCell::new(None),
        },
        owner_changed: Cell::new(false),
    };
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    assert!(
        admission
            .packages
            .iter()
            .all(|p| p.owners == crate::crate_release_registry::CrateRegistryOwnership::NewCrate)
    );
    let target = tempfile::tempdir()?;
    let first_archive = super::package_source(
        root,
        &intent,
        &admission.packages[0].source,
        target.path(),
        &request.source_sha,
        false,
        test_git(),
    )?;
    let server = SparseFixture::start("velnor_fixture_order_20261104_a", first_archive.clone())?;
    FIXTURE_SPARSE_INDEX.with(|value| *value.borrow_mut() = Some(server.url.clone()));
    let (identity, mut plan) = fixture_journal_plan(root, &intent, &request, &admission)?;
    plan.authentication = JournalAuthentication::BootstrapToken;
    plan.expected_tools.release_plz_sha256 = velnor_actions_mise::catalog::qualified_release_plz(
        &intent.release_plz_version,
        &intent.runner,
    )
    .ok_or("qualified release-plz")?
    .executable_sha256
    .into();
    set_workspace_journal_packages(&mut plan, &admission);
    for package in &mut plan.packages {
        package.owners = JournalOwnership::NewCrate;
    }
    let journal = JournalRegistryFake::default();
    record_source_bound_start(&journal, &identity, &plan, root, test_git())?;
    let input = CrateReleaseObservation {
        git: test_git(),
        journal: &journal,
        identity: &identity,
        journal_receipt: &plan,
        admission: &admission,
        intent: &intent,
        request: &request,
        github: &github,
        registry: &registry,
        source: root,
        sequence: 1,
    };
    execute_two_new_workspace_crates(&input, &first_archive, &server, change_owner)?;
    Ok(())
}

fn execute_two_new_workspace_crates(
    input: &crate::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        CreationWorkspaceGithub,
        CreationWorkspaceRegistry,
    >,
    first_archive: &[u8],
    server: &SparseFixture,
    change_owner: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::{UploadAttempt, coordinate_release};
    let admission = input.admission;
    let intent = input.intent;
    let request = input.request;
    let registry = input.registry;
    let github = input.github;
    let root = input.source;
    let uploads = RefCell::new(Vec::new());
    let result = coordinate_release(
        input,
        |name| crate::crate_release_semver::fixture_verified_semver(admission, name),
        |permit| {
            let rendered = super::fixture_upload_config(intent, &permit.package)?;
            let config: toml::Value = toml::from_str(&rendered).map_err(|_| err())?;
            let packages = config["package"].as_array().ok_or_else(err)?;
            assert_eq!(packages.len(), 1);
            assert_eq!(packages[0]["name"].as_str(), Some(permit.package.as_str()));
            super::plz::validate_upload(
                input,
                &permit,
                &velnor_actions_mise::command::CratePublishCredential::Bootstrap {
                    cargo_token: "fixture",
                },
            )?;
            uploads.borrow_mut().push(permit.package.clone());
            if permit.package.ends_with("_a") {
                *registry.inner.first.archive.borrow_mut() = first_archive.to_vec();
                registry.inner.first.visible.set(true);
            } else {
                let target = tempfile::tempdir().map_err(|_| OrchestratorError::Contract {
                    problem: "fixture".into(),
                })?;
                let archive = super::package_source(
                    root,
                    intent,
                    &admission.packages[1].source,
                    target.path(),
                    &request.source_sha,
                    false,
                    test_git(),
                )?;
                *registry.inner.second.borrow_mut() = Some(archive);
            }
            Ok(UploadAttempt::ExitedSuccess)
        },
        || {
            registry.inner.first.index_visible.set(true);
            server
                .index_visible
                .store(true, std::sync::atomic::Ordering::SeqCst);
            if change_owner {
                registry.owner_changed.set(true);
            }
        },
    );
    FIXTURE_SPARSE_INDEX.with(|value| *value.borrow_mut() = None);
    if change_owner {
        assert!(result.is_err());
        assert_eq!(&*uploads.borrow(), &["velnor_fixture_order_20261104_a"]);
        assert_eq!(&*github.0.writes.borrow(), &["tag-0", "release-0"]);
    } else {
        assert_eq!(result?.packages.len(), 2);
        assert_eq!(
            &*uploads.borrow(),
            &[
                "velnor_fixture_order_20261104_a",
                "velnor_fixture_order_20261104_b"
            ]
        );
        assert_eq!(
            &*github.0.writes.borrow(),
            &["tag-0", "release-0", "tag-1", "release-1"]
        );
    }
    Ok(())
}

#[cfg(unix)]
#[path = "crate_release_coordinator_rehearsal.rs"]
mod coordinator_rehearsal;

fn render_supplied_upload_config(bytes: &[u8], package: &str) -> Result<String, OrchestratorError> {
    let intent = crate::crate_release_runtime::parse_crate_release_intent(bytes)?;
    super::fixture_upload_config(&intent, package)
}

fn write_supplied_upload_config(
    input: &Path,
    package: &str,
    output: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write as _;
    let config = render_supplied_upload_config(&fs::read(input)?, package)?;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?
        .write_all(config.as_bytes())?;
    Ok(())
}

#[test]
fn supplied_intent_renders_actual_upload_config() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let (intent, _, _, _) = make_fixture(temp.path())?;
    let bytes = toml::to_string(&intent)?;
    let package = &intent.packages[0];
    let config = render_supplied_upload_config(bytes.as_bytes(), package)?;
    let parsed: toml::Value = toml::from_str(&config)?;
    assert_eq!(
        parsed["package"][0]["name"].as_str(),
        Some(package.as_str())
    );
    assert!(render_supplied_upload_config(bytes.as_bytes(), "not-selected").is_err());
    assert!(render_supplied_upload_config(b"invalid", package).is_err());
    let invalid = temp.path().join("invalid-intent");
    let output = temp.path().join("output");
    fs::write(&invalid, b"invalid")?;
    assert!(write_supplied_upload_config(&invalid, package, &output).is_err());
    assert!(!output.exists());
    if let Some(path) = std::env::var_os("VELNOR_TEST_UPLOAD_INTENT") {
        let package = std::env::var("VELNOR_TEST_UPLOAD_PACKAGE")?;
        let output = std::env::var_os("VELNOR_TEST_UPLOAD_OUTPUT").ok_or("output path")?;
        write_supplied_upload_config(Path::new(&path), &package, Path::new(&output))?;
    }
    Ok(())
}

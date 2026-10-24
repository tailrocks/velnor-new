use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::config::{HostConfig, RepositoryConnection};
use crate::error::HostError;
use crate::journal::{ConnectionJournal, EngineContext, Journal, fixture_profile};
use crate::launch::{SelectedEngineContext, verified_images::test_worker_images};

use super::bind_connection;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-host-setup-policy-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).map_err(|_| HostError::Journal)?;
        Ok(Self(path))
    }

    fn file(&self) -> PathBuf {
        self.0.join("journal.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.0);
    }
}

struct Fixture {
    _scratch: Scratch,
    journal: Journal,
    binding: RepositoryConnection,
    config: HostConfig,
    selected: SelectedEngineContext,
}

async fn fixture(generation: u64) -> Result<Fixture, HostError> {
    let scratch = Scratch::new()?;
    let journal = Journal::open(&scratch.file()).await?;
    journal.configure_storage_profile(fixture_profile()).await?;
    let images = test_worker_images();
    journal.bind_engine(images.engine_id()).await?;

    let config_text = crate::config_test_support::host_config(
        &[crate::config_test_support::basic_connection(
            "setup-policy",
            101,
            "owner/repository",
            "setup-policy",
            "setup-set",
        )],
        1,
    );
    let base = HostConfig::parse(&config_text)?;
    let binding = base.connections()[0].clone();
    let config = HostConfig::test_with_job_authority_policy_generation(&binding, generation)?;
    let selected = SelectedEngineContext::bind(
        &EngineContext {
            engine_id: images.engine_id().to_owned(),
            endpoint: config.docker.endpoint.clone(),
            platform: config.docker.platform.clone(),
        },
        &images,
    )?;
    Ok(Fixture {
        _scratch: scratch,
        journal,
        binding,
        config,
        selected,
    })
}

async fn open_scope(
    journal: &Journal,
    binding: &RepositoryConnection,
) -> Result<ConnectionJournal, HostError> {
    journal.connection_scope(binding).await
}

#[tokio::test]
async fn setup_installs_and_rechecks_the_configured_authority_policy() -> Result<(), HostError> {
    let test_fixture = fixture(1).await?;
    bind_connection(
        &test_fixture.journal,
        &test_fixture.binding,
        &test_fixture.config,
        &test_fixture.selected,
    )
    .await?;

    let policy = test_fixture
        .config
        .validated_job_authority_policy(&test_fixture.binding.connection_id)?
        .ok_or(HostError::Identity)?;
    let scoped = open_scope(&test_fixture.journal, &test_fixture.binding).await?;
    assert_eq!(
        scoped.test_active_authority_identity().await?,
        (policy.generation(), policy.policy_sha256().to_owned())
    );

    bind_connection(
        &test_fixture.journal,
        &test_fixture.binding,
        &test_fixture.config,
        &test_fixture.selected,
    )
    .await?;
    assert_eq!(
        scoped.test_active_authority_identity().await?,
        (policy.generation(), policy.policy_sha256().to_owned())
    );
    Ok(())
}

#[tokio::test]
async fn setup_rejects_missing_or_wrong_binding_policy_before_preparation() -> Result<(), HostError>
{
    let test_fixture = fixture(1).await?;
    let without_policy = HostConfig::parse(&crate::config_test_support::host_config(
        &[crate::config_test_support::basic_connection(
            "setup-policy",
            101,
            "owner/repository",
            "setup-policy",
            "setup-set",
        )],
        1,
    ))?;
    assert!(matches!(
        bind_connection(
            &test_fixture.journal,
            &test_fixture.binding,
            &without_policy,
            &test_fixture.selected,
        )
        .await,
        Err(HostError::Identity)
    ));
    let scoped = open_scope(&test_fixture.journal, &test_fixture.binding).await?;
    assert!(scoped.test_active_authority_identity().await.is_err());

    let wrong_fixture = fixture(1).await?;
    let wrong_text = crate::config_test_support::host_config(
        &[crate::config_test_support::basic_connection(
            "setup-policy",
            202,
            "owner/other-repository",
            "setup-policy",
            "setup-set",
        )],
        1,
    );
    let wrong_binding = HostConfig::parse(&wrong_text)?.connections()[0].clone();
    assert!(matches!(
        bind_connection(
            &wrong_fixture.journal,
            &wrong_binding,
            &wrong_fixture.config,
            &wrong_fixture.selected,
        )
        .await,
        Err(HostError::Identity)
    ));
    let wrong_scope = open_scope(&wrong_fixture.journal, &wrong_binding).await?;
    assert!(wrong_scope.test_active_authority_identity().await.is_err());
    Ok(())
}

#[tokio::test]
async fn setup_rejects_stale_policy_generation_without_replacing_active_policy()
-> Result<(), HostError> {
    let test_fixture = fixture(2).await?;
    bind_connection(
        &test_fixture.journal,
        &test_fixture.binding,
        &test_fixture.config,
        &test_fixture.selected,
    )
    .await?;
    let active = test_fixture
        .config
        .validated_job_authority_policy(&test_fixture.binding.connection_id)?
        .ok_or(HostError::Identity)?;

    let stale = HostConfig::test_with_job_authority_policy_generation(&test_fixture.binding, 1)?;
    assert!(matches!(
        bind_connection(
            &test_fixture.journal,
            &test_fixture.binding,
            &stale,
            &test_fixture.selected
        )
        .await,
        Err(HostError::Identity)
    ));
    let scoped = open_scope(&test_fixture.journal, &test_fixture.binding).await?;
    assert_eq!(
        scoped.test_active_authority_identity().await?,
        (active.generation(), active.policy_sha256().to_owned())
    );
    Ok(())
}

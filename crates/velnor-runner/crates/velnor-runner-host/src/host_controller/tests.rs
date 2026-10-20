use std::collections::HashMap;

use crate::config::RepositoryConnection;
use crate::config_test_support::{basic_connection, host_config};
use crate::{HostConfig, HostError};

#[path = "tests/header_fairness.rs"]
mod header_fairness;

use super::loop_driver::{
    TransitionEffects, apply_config_transition, requested_header_capacity,
    requested_header_capacity_after, retire_pending,
};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Event {
    ConfigureSharedBudget,
    Bind(String, String),
    Publish(Vec<String>),
    Retire(String),
}

#[derive(Default)]
struct FakeEffects {
    events: Vec<Event>,
    retire_drained: HashMap<String, bool>,
    fail_at: Option<&'static str>,
}

impl TransitionEffects for FakeEffects {
    async fn configure_shared_budget(&mut self, _config: &HostConfig) -> Result<(), HostError> {
        tokio::task::yield_now().await;
        self.events.push(Event::ConfigureSharedBudget);
        self.fail_if("configure")
    }

    async fn bind_connection(
        &mut self,
        binding: &RepositoryConnection,
        _config: &HostConfig,
    ) -> Result<(), HostError> {
        tokio::task::yield_now().await;
        self.events.push(Event::Bind(
            binding.connection_id.to_string(),
            binding.credential_ref.to_string(),
        ));
        self.fail_if("bind")
    }

    fn publish_config(&mut self, config: &HostConfig) -> Result<(), HostError> {
        self.events.push(Event::Publish(
            config
                .connections()
                .iter()
                .map(|binding| binding.connection_id.to_string())
                .collect(),
        ));
        self.fail_if("publish")
    }

    async fn retire_removed(&mut self, binding: &RepositoryConnection) -> Result<bool, HostError> {
        tokio::task::yield_now().await;
        let id = binding.connection_id.to_string();
        self.events.push(Event::Retire(id.clone()));
        self.fail_if("retire")?;
        Ok(self.retire_drained.get(&id).copied().unwrap_or(true))
    }
}

impl FakeEffects {
    fn fail_if(&self, stage: &'static str) -> Result<(), HostError> {
        if self.fail_at == Some(stage) {
            Err(HostError::Journal)
        } else {
            Ok(())
        }
    }
}

fn config(include_chainargos: bool) -> Result<HostConfig, HostError> {
    let essential = basic_connection(
        "essential",
        101,
        "donbeave/essential-mac",
        "essential",
        "essential-scale-set",
    );
    let mut connections = vec![essential];
    if include_chainargos {
        connections.push(basic_connection(
            "chainargos",
            202,
            "ChainArgos/java-monorepo",
            "chainargos",
            "chainargos-sea-scale-set",
        ));
    }
    HostConfig::parse(&host_config(&connections, 2))
}

fn run<F>(future: F) -> F::Output
where
    F: std::future::Future,
{
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime")
        .block_on(future)
}

#[test]
fn multiple_connections_share_one_budget_and_keep_separate_credentials() -> Result<(), HostError> {
    let next = config(true)?;
    let mut effects = FakeEffects::default();
    assert_eq!(
        run(apply_config_transition(
            &config(false)?,
            &next,
            &mut effects
        ))?,
        []
    );

    assert_eq!(
        effects.events,
        [
            Event::ConfigureSharedBudget,
            Event::Bind(
                "essential".to_owned(),
                "keychain:com.tailrocks.velnor.host/essential".to_owned(),
            ),
            Event::Bind(
                "chainargos".to_owned(),
                "keychain:com.tailrocks.velnor.host/chainargos".to_owned(),
            ),
            Event::Publish(vec!["essential".to_owned(), "chainargos".to_owned()]),
        ]
    );
    Ok(())
}

#[test]
fn removal_publishes_absence_before_retiring_only_the_removed_binding() -> Result<(), HostError> {
    let previous = config(true)?;
    let next = config(false)?;
    let mut effects = FakeEffects::default();
    let mut pending = run(apply_config_transition(&previous, &next, &mut effects))?;
    run(retire_pending(&mut pending, &mut effects))?;
    assert_eq!(pending, []);

    assert_eq!(
        effects.events,
        [
            Event::ConfigureSharedBudget,
            Event::Bind(
                "essential".to_owned(),
                "keychain:com.tailrocks.velnor.host/essential".to_owned(),
            ),
            Event::Publish(vec!["essential".to_owned()]),
            Event::Retire("chainargos".to_owned()),
        ]
    );
    Ok(())
}

#[test]
fn failed_scope_setup_does_not_publish_or_retire() -> Result<(), HostError> {
    let previous = config(false)?;
    let next = config(true)?;
    let mut effects = FakeEffects {
        fail_at: Some("bind"),
        ..FakeEffects::default()
    };

    assert_eq!(
        run(apply_config_transition(&previous, &next, &mut effects)),
        Err(HostError::Journal)
    );
    assert!(matches!(effects.events.last(), Some(Event::Bind(_, _))));
    assert!(
        !effects
            .events
            .iter()
            .any(|event| matches!(event, Event::Publish(_) | Event::Retire(_)))
    );
    Ok(())
}

#[test]
fn undrained_removed_binding_keeps_the_transition_pending() -> Result<(), HostError> {
    let previous = config(true)?;
    let next = config(false)?;
    let mut effects = FakeEffects {
        retire_drained: HashMap::from([("chainargos".to_owned(), false)]),
        ..FakeEffects::default()
    };

    let mut pending = run(apply_config_transition(&previous, &next, &mut effects))?;
    run(retire_pending(&mut pending, &mut effects))?;
    assert_eq!(pending.len(), 1);
    assert_eq!(
        effects.events.last(),
        Some(&Event::Retire("chainargos".to_owned()))
    );
    assert!(
        effects.events.iter().any(|event| {
            matches!(event, Event::Publish(ids) if ids == &["essential".to_owned()])
        })
    );
    Ok(())
}

#[test]
fn retirement_error_keeps_the_removed_binding_for_retry() -> Result<(), HostError> {
    let previous = config(true)?;
    let next = config(false)?;
    let mut effects = FakeEffects::default();
    let mut pending = run(apply_config_transition(&previous, &next, &mut effects))?;
    effects.fail_at = Some("retire");

    assert_eq!(
        run(retire_pending(&mut pending, &mut effects)),
        Err(HostError::Journal)
    );
    assert_eq!(pending.len(), 1);
    Ok(())
}

#[test]
fn stable_connection_id_cannot_change_binding_or_engine() -> Result<(), HostError> {
    let previous = config(false)?;
    let changed = HostConfig::parse(&host_config(
        &[basic_connection(
            "essential",
            101,
            "donbeave/essential-mac",
            "other-credential",
            "essential-scale-set",
        )],
        2,
    ))?;

    let mut effects = FakeEffects::default();
    assert_eq!(
        run(apply_config_transition(&previous, &changed, &mut effects)),
        Err(HostError::Config)
    );
    assert_eq!(effects.events, []);

    let mut changed_engine = previous.clone();
    changed_engine.docker.endpoint = "unix:///tmp/other-docker.sock".to_owned();
    assert_eq!(
        run(apply_config_transition(
            &previous,
            &changed_engine,
            &mut effects,
        )),
        Err(HostError::Config)
    );
    assert_eq!(effects.events, []);
    Ok(())
}

#[test]
fn session_header_targets_share_the_configured_job_budget() -> Result<(), HostError> {
    let config = HostConfig::parse(&host_config(
        &[
            basic_connection("essential", 101, "owner/one", "one", "one-set"),
            basic_connection("chainargos", 202, "owner/two", "two", "two-set"),
            basic_connection("third", 303, "owner/three", "three", "three-set"),
        ],
        5,
    ))?;
    let targets = config
        .connections()
        .iter()
        .map(|binding| requested_header_capacity(&config, binding))
        .collect::<Result<Vec<_>, _>>()?;

    assert_eq!(targets, [2, 2, 1]);
    assert_eq!(targets.iter().sum::<u32>(), config.max_jobs());
    Ok(())
}

#[test]
fn session_header_targets_allow_zero_when_connections_exceed_capacity() -> Result<(), HostError> {
    let config = HostConfig::parse(&host_config(
        &[
            basic_connection("essential", 101, "owner/one", "one", "one-set"),
            basic_connection("chainargos", 202, "owner/two", "two", "two-set"),
            basic_connection("third", 303, "owner/three", "three", "three-set"),
        ],
        2,
    ))?;
    let targets = config
        .connections()
        .iter()
        .map(|binding| requested_header_capacity(&config, binding))
        .collect::<Result<Vec<_>, _>>()?;

    assert_eq!(targets, [1, 1, 0]);
    assert_eq!(targets.iter().sum::<u32>(), config.max_jobs());
    Ok(())
}

#[test]
fn oversubscribed_header_targets_follow_the_durable_connection_cursor() -> Result<(), HostError> {
    let config = HostConfig::parse(&host_config(
        &[
            basic_connection("gamma", 303, "owner/three", "three", "three-set"),
            basic_connection("alpha", 101, "owner/one", "one", "one-set"),
            basic_connection("beta", 202, "owner/two", "two", "two-set"),
        ],
        1,
    ))?;
    let targets = |cursor: Option<&crate::config::ConnectionId>| {
        config
            .connections()
            .iter()
            .map(|binding| requested_header_capacity_after(&config, binding, cursor))
            .collect::<Result<Vec<_>, _>>()
    };

    assert_eq!(targets(None)?, [0, 1, 0]);
    assert_eq!(
        targets(Some(&crate::config::ConnectionId::parse("alpha")?))?,
        [0, 0, 1]
    );
    assert_eq!(
        targets(Some(&crate::config::ConnectionId::parse("beta")?))?,
        [1, 0, 0]
    );
    Ok(())
}

use crate::config_test_support::{basic_connection, connection, host_config};
use crate::{
    ConnectionId, HostConfig, HostError, KeychainReference, PullRequestRule, ScaleSetSelector,
    SourcePolicy, SourcePolicyMode, WorkflowEvent,
};

fn parsed(connection: String) -> Result<HostConfig, HostError> {
    HostConfig::parse(&host_config(&[connection], 1))
}

#[test]
fn schema_two_accepts_multiple_connections_with_one_host_budget() -> Result<(), HostError> {
    let configs = [
        basic_connection(
            "essential",
            101,
            "donbeave/essential-mac",
            "essential",
            "essential-scale-set",
        ),
        basic_connection(
            "chainargos",
            202,
            "ChainArgos/java-monorepo",
            "chainargos",
            "chainargos-sea-scale-set",
        ),
    ];
    let config = HostConfig::parse(&host_config(&configs, 1))?;
    assert_eq!(config.schema, 2);
    assert_eq!(config.connections().len(), 2);
    assert_eq!(config.host.max_jobs, 1);
    assert_eq!(
        config
            .connection(&ConnectionId::parse("chainargos")?)
            .map(|binding| binding.repository.as_str()),
        Some("ChainArgos/java-monorepo")
    );
    assert_eq!(
        config.connections()[0].scale_set.labels(),
        ["velnor", "essential-scale-set"]
    );
    Ok(())
}

#[test]
fn release_reader_reference_is_optional_for_old_config_and_round_trips_when_set()
-> Result<(), HostError> {
    let mut config = parsed(basic_connection(
        "essential",
        101,
        "donbeave/essential-mac",
        "essential",
        "essential-scale-set",
    ))?;
    assert_eq!(config.release_read_credential_ref(), None);
    assert!(!config.to_toml()?.contains("[release_read]"));

    let reference = KeychainReference::parse("keychain:com.tailrocks.velnor.host/release-read")?;
    config.set_release_read_credential_ref(reference.clone())?;
    assert_eq!(config.release_read_credential_ref(), Some(&reference));
    let encoded = config.to_toml()?;
    assert!(encoded.contains("[release_read]"));
    assert!(
        encoded.contains("credential_ref = \"keychain:com.tailrocks.velnor.host/release-read\"")
    );
    let reopened = HostConfig::parse(&encoded)?;
    assert_eq!(reopened.release_read_credential_ref(), Some(&reference));

    let repository_ref = config.connections()[0].credential_ref.clone();
    assert!(
        config
            .set_release_read_credential_ref(repository_ref)
            .is_err()
    );
    assert_eq!(config.release_read_credential_ref(), Some(&reference));
    Ok(())
}

#[test]
fn schema_one_and_the_single_github_binding_are_rejected() {
    let schema_one = "schema = 1\n[github]\nrepository = \"owner/repo\"\n";
    assert!(HostConfig::parse(schema_one).is_err());
    let no_connections = host_config(&[], 1);
    assert!(HostConfig::parse(&no_connections).is_err());
}

#[test]
fn repository_ids_repositories_credentials_and_ids_are_unique() {
    let first = basic_connection("first", 10, "owner/one", "one", "one-scale-set");
    let duplicate_id = basic_connection("first", 11, "owner/two", "two", "two-scale-set");
    let duplicate_repository_id =
        basic_connection("second", 10, "owner/two", "two", "two-scale-set");
    let duplicate_repository = basic_connection("second", 11, "OWNER/ONE", "two", "two-scale-set");
    let duplicate_credential = basic_connection("second", 11, "owner/two", "one", "two-scale-set");
    for second in [
        duplicate_id,
        duplicate_repository_id,
        duplicate_repository,
        duplicate_credential,
    ] {
        assert!(HostConfig::parse(&host_config(&[first.clone(), second], 1)).is_err());
    }
}

#[test]
fn connection_ids_and_scale_set_labels_follow_the_generator_grammar() -> Result<(), HostError> {
    assert_eq!(
        ConnectionId::parse("chainargos.sea_1")?.as_str(),
        "chainargos.sea_1"
    );
    for invalid in ["", "Upper", "-edge", "edge-", "double..dot", "has space"] {
        assert!(ConnectionId::parse(invalid).is_err(), "{invalid}");
    }
    let selector = ScaleSetSelector::try_new(
        "chainargos-scale-set",
        &[
            "extra".to_owned(),
            "chainargos-scale-set".to_owned(),
            "velnor".to_owned(),
        ],
    )?;
    assert_eq!(
        selector.labels(),
        ["velnor", "chainargos-scale-set", "extra"]
    );
    for (name, labels) in [
        (
            "ubuntu-26.04",
            vec!["velnor".to_owned(), "ubuntu-26.04".to_owned()],
        ),
        (
            "custom",
            vec![
                "velnor".to_owned(),
                "custom".to_owned(),
                "custom".to_owned(),
            ],
        ),
        ("custom", vec!["custom".to_owned()]),
        ("bad name", vec!["velnor".to_owned(), "bad name".to_owned()]),
    ] {
        assert!(ScaleSetSelector::try_new(name, &labels).is_err());
    }
    Ok(())
}

#[test]
fn chainargos_policy_pins_branch_and_pull_request_contexts() -> Result<(), HostError> {
    // Head repository ID 202 is synthetic for this fixture; the base ID is bound.
    let policy = concat!(
        "branches = [\"sea/integration\"]\n",
        "tag_prefixes = []\n",
        "events = [\"push\", \"pull_request\", \"merge_group\"]\n",
        "pull_request_rules = [",
        "{ base_repository_id = 829618808, base_branch = \"sea/integration\", head_repository_id = 829618808, head_branch = \"sea/integration\" },",
        "{ number = 2109, base_repository_id = 829618808, base_branch = \"main\", head_repository_id = 202, head_branch = \"sea/integration\" }",
        "]\n",
    );
    let text = connection(
        "chainargos",
        829_618_808,
        "ChainArgos/java-monorepo",
        "chainargos",
        "chainargos-sea-scale-set",
        policy,
    );
    let config = parsed(text)?;
    let source = &config.connections()[0].source_policy;
    assert_eq!(source.branches, ["sea/integration"]);
    assert!(source.tag_prefixes.is_empty());
    assert!(!source.events.contains(&WorkflowEvent::Schedule));
    assert!(!source.events.contains(&WorkflowEvent::Release));
    assert_eq!(source.pull_request_rules.len(), 2);
    assert!(source.pull_request_rules.iter().any(|rule| {
        rule.number == Some(2109)
            && rule.base_branch == "main"
            && rule.head_branch == "sea/integration"
            && rule.head_repository_id == 202
    }));
    assert!(source.pull_request_rules.iter().any(|rule| {
        rule.number.is_none()
            && rule.base_branch == "sea/integration"
            && rule.head_branch == "sea/integration"
    }));
    Ok(())
}

#[test]
fn pull_request_rules_require_the_bound_base_repository() {
    let bad_policy = concat!(
        "branches = []\n",
        "tag_prefixes = []\n",
        "events = [\"pull_request\"]\n",
        "pull_request_rules = [{ number = 2109, base_repository_id = 99, base_branch = \"main\", head_repository_id = 829618808, head_branch = \"sea/integration\" }]\n",
    );
    let text = connection(
        "chainargos",
        829_618_808,
        "ChainArgos/java-monorepo",
        "chainargos",
        "chainargos-sea-scale-set",
        bad_policy,
    );
    assert!(parsed(text).is_err());
}

#[test]
fn tag_publication_requires_explicit_prefixes_and_event_rules() -> Result<(), HostError> {
    let policy = SourcePolicy::try_new(
        SourcePolicyMode::BranchRestricted,
        Vec::new(),
        vec!["v".to_owned()],
        vec![WorkflowEvent::Push, WorkflowEvent::Release],
        Vec::new(),
    )?;
    assert_eq!(policy.tag_prefixes, ["v"]);
    assert!(
        SourcePolicy::try_new(
            SourcePolicyMode::BranchRestricted,
            Vec::new(),
            Vec::new(),
            vec![WorkflowEvent::Release],
            Vec::new(),
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn repository_mode_is_explicit_and_has_no_ref_restrictions() -> Result<(), HostError> {
    let policy = SourcePolicy::try_new(
        SourcePolicyMode::Repository,
        Vec::new(),
        Vec::new(),
        vec![WorkflowEvent::PullRequest, WorkflowEvent::Push],
        Vec::new(),
    )?;
    assert_eq!(policy.mode, SourcePolicyMode::Repository);
    assert!(
        SourcePolicy::try_new(
            SourcePolicyMode::Repository,
            vec!["main".to_owned()],
            Vec::new(),
            vec![WorkflowEvent::Push],
            Vec::new(),
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn canonical_identity_and_toml_round_trip_keep_the_full_binding() -> Result<(), HostError> {
    let text = host_config(
        &[basic_connection(
            "essential",
            101,
            "donbeave/essential-mac",
            "essential",
            "essential-scale-set",
        )],
        1,
    );
    let config = HostConfig::parse(&text)?;
    let encoded = config.to_toml()?;
    let round_trip = HostConfig::parse(&encoded)?;
    assert_eq!(round_trip, config);
    let bytes = config.connections()[0].canonical_identity_bytes()?;
    assert!(String::from_utf8_lossy(&bytes).contains("source_policy"));
    assert!(encoded.contains("keychain:com.tailrocks.velnor.host/essential"));
    assert!(encoded.contains("mode = \"branch_restricted\""));
    Ok(())
}

#[test]
fn direct_policy_rules_require_positive_ids_and_exact_refs() {
    let valid = PullRequestRule {
        number: Some(2109),
        base_repository_id: 202,
        base_branch: "main".to_owned(),
        head_repository_id: 202,
        head_branch: "sea/integration".to_owned(),
    };
    assert!(
        SourcePolicy::try_new(
            SourcePolicyMode::BranchRestricted,
            vec!["sea/integration".to_owned()],
            Vec::new(),
            vec![WorkflowEvent::PullRequest],
            vec![valid],
        )
        .is_ok()
    );
    let invalid = PullRequestRule {
        number: Some(0),
        base_repository_id: 202,
        base_branch: "main".to_owned(),
        head_repository_id: 202,
        head_branch: "sea/integration".to_owned(),
    };
    assert!(
        SourcePolicy::try_new(
            SourcePolicyMode::BranchRestricted,
            Vec::new(),
            Vec::new(),
            vec![WorkflowEvent::PullRequest],
            vec![invalid],
        )
        .is_err()
    );
}

//! Tests for bounded, repository-scoped discovery credential bootstrap.

#[path = "common/discovery.rs"]
mod discovery_test_support;

use discovery_test_support::{
    ADMIN_TOKEN, HOST_CREDENTIAL, Intents, OWNER, REGISTRATION_TOKEN, REPOSITORY, Script, header,
    reply, repository_admin,
};
use std::rc::Rc;
use velnor_runner_github::{
    DiscoveryCredentialOutcome, DiscoveryCredentialStep, DiscoveryIntentId, Method,
    RepositoryDiscoveryAdmin, RunnerGroup, ScaleSetFound, SessionError, TransportFail, WireError,
    exchange_repository_discovery_admin_once, issue_repository_discovery_token,
    read_repository_admin_evidence,
};

fn successful_script() -> (Script, Intents) {
    let events = Rc::default();
    let script = Script::with_events(
        vec![
            reply(200, &repository_admin(true, Some(true))),
            reply(201, &format!(r#"{{"token":"{REGISTRATION_TOKEN}"}}"#)),
            reply(
                200,
                &format!(r#"{{"url":"https://actions.example/org","token":"{ADMIN_TOKEN}"}}"#),
            ),
            reply(
                200,
                r#"{"count":1,"value":[{"id":1,"name":"Default","isDefaultGroup":true}]}"#,
            ),
            reply(200, r#"{"count":0,"value":[]}"#),
        ],
        Rc::clone(&events),
    );
    (script, Intents::new(events))
}

fn open_discovery_admin(
    script: &mut Script,
    intents: &mut Intents,
) -> Result<RepositoryDiscoveryAdmin, String> {
    let evidence = read_repository_admin_evidence(script, OWNER, REPOSITORY, HOST_CREDENTIAL)
        .map_err(|error| format!("read repository metadata: {error:?}"))?;
    assert_eq!(evidence.repository_id(), 829_618_808);
    assert_eq!(evidence.full_name(), "ChainArgos/java-monorepo");
    assert!(!format!("{evidence:?}").contains(HOST_CREDENTIAL));
    let registration = issue_repository_discovery_token(script, evidence, HOST_CREDENTIAL, intents)
        .map_err(|error| format!("issue repository token: {error:?}"))?;
    assert!(!format!("{registration:?}").contains(REGISTRATION_TOKEN));
    let admin = exchange_repository_discovery_admin_once(script, registration, intents)
        .map_err(|error| format!("exchange admin: {error:?}"))?;
    assert!(!format!("{admin:?}").contains(ADMIN_TOKEN));
    Ok(admin)
}

fn assert_intent_order(intents: &Intents) -> Result<(), String> {
    assert_eq!(
        intents.before,
        [
            (
                DiscoveryCredentialStep::RepositoryRegistrationToken,
                829_618_808,
                "ChainArgos/java-monorepo".to_owned(),
                DiscoveryIntentId::new(1).ok_or("intent id")?,
            ),
            (
                DiscoveryCredentialStep::ActionsAdminExchange,
                829_618_808,
                "ChainArgos/java-monorepo".to_owned(),
                DiscoveryIntentId::new(2).ok_or("intent id")?,
            ),
        ]
    );
    assert_eq!(
        intents.outcomes,
        [
            (
                DiscoveryIntentId::new(1).ok_or("intent id")?,
                DiscoveryCredentialOutcome::Succeeded,
            ),
            (
                DiscoveryIntentId::new(2).ok_or("intent id")?,
                DiscoveryCredentialOutcome::Succeeded,
            ),
        ]
    );
    Ok(())
}

fn assert_bootstrap_event_order(script: &Script) {
    assert_eq!(
        *script.events.borrow(),
        [
            "bind-api",
            "request:Get:repos/ChainArgos/java-monorepo",
            "bind-api",
            "intent:RepositoryRegistrationToken",
            "request:Post:/repos/ChainArgos/java-monorepo/actions/runners/registration-token",
            "outcome:1:Succeeded",
            "bind-api",
            "intent:ActionsAdminExchange",
            "request:Post:/actions/runner-registration",
            "outcome:2:Succeeded",
            "bind-actions",
            "request:Get:_apis/runtime/runnergroups",
            "bind-actions",
            "request:Get:_apis/runtime/runnerscalesets",
        ]
    );
}

fn assert_repository_requests(script: &Script) {
    let metadata = &script.seen[0];
    assert_eq!(metadata.method, Method::Get);
    assert_eq!(metadata.path, "repos/ChainArgos/java-monorepo");
    assert_eq!(metadata.query, None);
    assert_eq!(metadata.body.len(), 0);
    assert_eq!(
        header(metadata, "Authorization"),
        Some("Bearer host-credential-canary")
    );

    let registration = &script.seen[1];
    assert_eq!(registration.method, Method::Post);
    assert_eq!(
        registration.path,
        "/repos/ChainArgos/java-monorepo/actions/runners/registration-token"
    );
    assert_eq!(
        header(registration, "Authorization"),
        Some("Bearer host-credential-canary")
    );

    let exchange = &script.seen[2];
    assert_eq!(exchange.method, Method::Post);
    assert_eq!(exchange.path, "/actions/runner-registration");
    assert_eq!(
        String::from_utf8_lossy(&exchange.body),
        r#"{"url":"https://github.com/ChainArgos/java-monorepo","runner_event":"register"}"#
    );
    assert_eq!(
        header(exchange, "Authorization"),
        Some("RemoteAuth registration-token-canary")
    );
}

fn assert_actions_requests(script: &Script) {
    assert_eq!(script.seen[3].method, Method::Get);
    assert_eq!(script.seen[3].path, "_apis/runtime/runnergroups");
    assert_eq!(
        header(&script.seen[3], "Authorization"),
        Some("Bearer admin-token-canary")
    );
    assert_eq!(script.seen[4].method, Method::Get);
    assert_eq!(script.seen[4].path, "_apis/runtime/runnerscalesets");
    assert_eq!(
        script.seen[4].query.as_deref(),
        Some("api-version=6.0-preview&name=ubuntu-26.04-scale-set&runnerGroupId=1")
    );
    assert_eq!(
        header(&script.seen[4], "Authorization"),
        Some("Bearer admin-token-canary")
    );
}

#[test]
fn discovery_bootstrap_is_repo_scoped_one_shot_and_secret_safe() -> Result<(), String> {
    let (mut script, mut intents) = successful_script();
    let admin = open_discovery_admin(&mut script, &mut intents)?;
    assert_eq!(admin.service_url(), "https://actions.example/org");
    let groups = admin
        .list_runner_groups(&mut script)
        .map_err(|error| format!("list groups: {error:?}"))?;
    assert_eq!(
        groups,
        [RunnerGroup {
            id: 1,
            name: "Default".to_owned(),
            is_default: true,
        }]
    );
    let missing = admin
        .get_existing_product_scale_set(&mut script, 1, "ubuntu-26.04-scale-set")
        .map_err(|error| format!("get existing set: {error:?}"))?;
    assert_eq!(missing, ScaleSetFound::NotFound);
    assert_eq!(script.seen.len(), 5);
    assert_eq!(script.api_bindings, 3);
    assert_eq!(
        script.actions_bindings,
        [
            "https://actions.example/org".to_owned(),
            "https://actions.example/org".to_owned(),
        ]
    );
    assert_intent_order(&intents)?;
    assert_bootstrap_event_order(&script);
    assert_repository_requests(&script);
    assert_actions_requests(&script);
    Ok(())
}

#[test]
fn untrusted_repository_metadata_stops_before_registration_post() {
    for metadata in [
        repository_admin(false, Some(true)),
        repository_admin(true, None),
        repository_admin(true, Some(false)),
    ] {
        let mut script = Script::new(vec![reply(200, &metadata), reply(201, "{}")]);
        assert_eq!(
            read_repository_admin_evidence(&mut script, OWNER, REPOSITORY, HOST_CREDENTIAL)
                .map(|_| ()),
            Err(SessionError::Wire(WireError::RegistrationRejected))
        );
        assert_eq!(script.seen.len(), 1);
        assert_eq!(script.seen[0].method, Method::Get);
    }
}

#[test]
fn refused_origin_binding_prevents_request_with_credentials() {
    let mut script = Script::new(vec![reply(200, &repository_admin(true, Some(true)))]);
    script.reject_binding = true;
    assert_eq!(
        read_repository_admin_evidence(&mut script, OWNER, REPOSITORY, HOST_CREDENTIAL).map(|_| ()),
        Err(SessionError::Wire(WireError::RegistrationRejected))
    );
    assert_eq!(script.seen.len(), 0);
    assert_eq!(script.api_bindings, 0);
}

#[test]
fn intent_must_be_persisted_before_registration_post() -> Result<(), String> {
    let mut script = Script::new(vec![reply(200, &repository_admin(true, Some(true)))]);
    let evidence = read_repository_admin_evidence(&mut script, OWNER, REPOSITORY, HOST_CREDENTIAL)
        .map_err(|error| format!("read repository metadata: {error:?}"))?;
    let mut intents = Intents::new(Rc::clone(&script.events));
    intents.fail_before = true;
    assert_eq!(
        issue_repository_discovery_token(&mut script, evidence, HOST_CREDENTIAL, &mut intents)
            .map(|_| ()),
        Err(SessionError::Uncertain)
    );
    assert_eq!(script.seen.len(), 1);
    assert_eq!(script.seen[0].method, Method::Get);
    Ok(())
}

#[test]
fn failed_outcome_write_keeps_successful_registration_post_uncertain() -> Result<(), String> {
    let mut script = Script::new(vec![
        reply(200, &repository_admin(true, Some(true))),
        reply(201, &format!(r#"{{"token":"{REGISTRATION_TOKEN}"}}"#)),
    ]);
    let evidence = read_repository_admin_evidence(&mut script, OWNER, REPOSITORY, HOST_CREDENTIAL)
        .map_err(|error| format!("read repository metadata: {error:?}"))?;
    let mut intents = Intents::new(Rc::clone(&script.events));
    intents.fail_outcome = true;
    assert_eq!(
        issue_repository_discovery_token(&mut script, evidence, HOST_CREDENTIAL, &mut intents)
            .map(|_| ()),
        Err(SessionError::Uncertain)
    );
    assert_eq!(script.seen.len(), 2);
    assert_eq!(script.seen[1].method, Method::Post);
    assert_eq!(intents.outcomes.len(), 0);
    Ok(())
}

#[test]
fn lost_registration_post_response_is_uncertain_and_not_replayed() -> Result<(), String> {
    let mut script = Script::from_results(
        vec![
            Ok(reply(200, &repository_admin(true, Some(true)))),
            Err(TransportFail::Timeout),
            Ok(reply(
                201,
                &format!(r#"{{"token":"{REGISTRATION_TOKEN}"}}"#),
            )),
        ],
        Rc::default(),
    );
    let evidence = read_repository_admin_evidence(&mut script, OWNER, REPOSITORY, HOST_CREDENTIAL)
        .map_err(|error| format!("read repository metadata: {error:?}"))?;
    let mut intents = Intents::new(Rc::clone(&script.events));
    assert_eq!(
        issue_repository_discovery_token(&mut script, evidence, HOST_CREDENTIAL, &mut intents)
            .map(|_| ()),
        Err(SessionError::Uncertain)
    );
    assert_eq!(script.seen.len(), 2);
    assert_eq!(script.seen[1].method, Method::Post);
    assert_eq!(
        intents.outcomes,
        [(
            DiscoveryIntentId::new(1).ok_or("intent id")?,
            DiscoveryCredentialOutcome::Uncertain,
        )]
    );
    Ok(())
}

#[test]
fn malformed_successful_registration_post_is_uncertain() -> Result<(), String> {
    let mut script = Script::new(vec![
        reply(200, &repository_admin(true, Some(true))),
        reply(201, r#"{"token":null}"#),
    ]);
    let evidence = read_repository_admin_evidence(&mut script, OWNER, REPOSITORY, HOST_CREDENTIAL)
        .map_err(|error| format!("read repository metadata: {error:?}"))?;
    let mut intents = Intents::new(Rc::clone(&script.events));
    assert_eq!(
        issue_repository_discovery_token(&mut script, evidence, HOST_CREDENTIAL, &mut intents)
            .map(|_| ()),
        Err(SessionError::Uncertain)
    );
    assert_eq!(script.seen.len(), 2);
    Ok(())
}

#[test]
fn admin_exchange_401_is_not_retried() -> Result<(), String> {
    let mut script = Script::new(vec![
        reply(200, &repository_admin(true, Some(true))),
        reply(201, &format!(r#"{{"token":"{REGISTRATION_TOKEN}"}}"#)),
        reply(401, ""),
        reply(
            200,
            r#"{"url":"https://actions.example/org","token":"late"}"#,
        ),
    ]);
    let evidence = read_repository_admin_evidence(&mut script, OWNER, REPOSITORY, HOST_CREDENTIAL)
        .map_err(|error| format!("read repository metadata: {error:?}"))?;
    let mut intents = Intents::new(Rc::clone(&script.events));
    let registration =
        issue_repository_discovery_token(&mut script, evidence, HOST_CREDENTIAL, &mut intents)
            .map_err(|error| format!("issue repository token: {error:?}"))?;
    assert!(
        exchange_repository_discovery_admin_once(&mut script, registration, &mut intents).is_err()
    );
    assert_eq!(script.seen.len(), 3);
    assert_eq!(script.seen[2].method, Method::Post);
    Ok(())
}

#[test]
fn refused_actions_origin_prevents_admin_token_get() -> Result<(), String> {
    let mut script = Script::new(vec![
        reply(200, &repository_admin(true, Some(true))),
        reply(201, &format!(r#"{{"token":"{REGISTRATION_TOKEN}"}}"#)),
        reply(
            200,
            r#"{"url":"https://unapproved.example/tenant","token":"secret-admin-canary"}"#,
        ),
        reply(200, r#"{"count":0,"value":[]}"#),
    ]);
    let evidence = read_repository_admin_evidence(&mut script, OWNER, REPOSITORY, HOST_CREDENTIAL)
        .map_err(|error| format!("read repository metadata: {error:?}"))?;
    let mut intents = Intents::new(Rc::clone(&script.events));
    let registration =
        issue_repository_discovery_token(&mut script, evidence, HOST_CREDENTIAL, &mut intents)
            .map_err(|error| format!("issue repository token: {error:?}"))?;
    let admin = exchange_repository_discovery_admin_once(&mut script, registration, &mut intents)
        .map_err(|error| format!("exchange admin: {error:?}"))?;
    script.reject_actions_binding = true;
    assert_eq!(
        admin.list_runner_groups(&mut script),
        Err(SessionError::Wire(WireError::RegistrationRejected))
    );
    assert_eq!(script.seen.len(), 3);
    assert_eq!(script.actions_bindings.len(), 0);
    Ok(())
}

#[test]
fn service_metadata_api_does_not_accept_unbounded_route_from_url() -> Result<(), String> {
    let mut script = Script::new(vec![
        reply(200, &repository_admin(true, Some(true))),
        reply(201, &format!(r#"{{"token":"{REGISTRATION_TOKEN}"}}"#)),
        reply(
            200,
            r#"{"url":"https://actions.example/org","token":"admin-token"}"#,
        ),
    ]);
    let evidence = read_repository_admin_evidence(&mut script, OWNER, REPOSITORY, HOST_CREDENTIAL)
        .map_err(|error| format!("read repository metadata: {error:?}"))?;
    let mut intents = Intents::new(Rc::clone(&script.events));
    let registration =
        issue_repository_discovery_token(&mut script, evidence, HOST_CREDENTIAL, &mut intents)
            .map_err(|error| format!("issue repository token: {error:?}"))?;
    let admin = exchange_repository_discovery_admin_once(&mut script, registration, &mut intents)
        .map_err(|error| format!("exchange admin: {error:?}"))?;
    assert_eq!(
        admin.get_existing_product_scale_set(&mut script, 1, "not-a-product"),
        Err(SessionError::Wire(WireError::RegistrationRejected))
    );
    assert_eq!(script.seen.len(), 3);
    Ok(())
}

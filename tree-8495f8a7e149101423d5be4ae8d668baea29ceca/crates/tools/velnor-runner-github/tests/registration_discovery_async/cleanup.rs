use super::*;
use velnor_runner_github::{
    RepositorySessionCleanupBinding, RepositorySessionCleanupExpectation,
    RepositorySessionCleanupOutcome, RepositorySessionCloseClaim,
};

const GROUPS: &str = r#"{"count":1,"value":[{"id":1,"name":"Default","isDefaultGroup":true}]}"#;
const SET: &str = r#"{"count":1,"value":[{"id":3,"name":"ubuntu-26.04-scale-set","labels":[{"name":"velnor","type":"System"},{"name":"ubuntu-26.04-scale-set","type":"System"}],"runnerSetting":{"disableUpdate":true}}]}"#;

fn binding() -> RepositorySessionCleanupBinding {
    RepositorySessionCleanupBinding::from_expected_fields(RepositorySessionCleanupExpectation {
        destination: "scale-set-controller",
        registration_scope: "repository",
        scope_name: "ChainArgos/java-monorepo",
        target_repository_id: 829_618_808,
        target_repository_full_name: "ChainArgos/java-monorepo",
        runner_group_id: 1,
        runner_group_name: "Default",
        scale_set_id: 3,
        scale_set_name: "ubuntu-26.04-scale-set",
    })
    .expect("exact repository cleanup binding")
}

struct TestClaim {
    intent_id: i64,
    destination: String,
    registration_scope: String,
    scope_name: String,
    target_repository_id: i64,
    target_repository_full_name: String,
    runner_group_id: i64,
    runner_group_name: String,
    scale_set_id: i64,
    scale_set_name: String,
    session_id: String,
    delete_attempted: bool,
}

impl TestClaim {
    fn repository(session_id: &str) -> Self {
        Self {
            intent_id: 7,
            destination: "scale-set-controller".to_owned(),
            registration_scope: "repository".to_owned(),
            scope_name: "ChainArgos/java-monorepo".to_owned(),
            target_repository_id: 829_618_808,
            target_repository_full_name: "ChainArgos/java-monorepo".to_owned(),
            runner_group_id: 1,
            runner_group_name: "Default".to_owned(),
            scale_set_id: 3,
            scale_set_name: "ubuntu-26.04-scale-set".to_owned(),
            session_id: session_id.to_owned(),
            delete_attempted: false,
        }
    }
}

impl RepositorySessionCloseClaim for TestClaim {
    fn intent_id(&self) -> i64 {
        self.intent_id
    }

    fn destination(&self) -> &str {
        &self.destination
    }

    fn registration_scope(&self) -> &str {
        &self.registration_scope
    }

    fn scope_name(&self) -> &str {
        &self.scope_name
    }

    fn target_repository_id(&self) -> i64 {
        self.target_repository_id
    }

    fn target_repository_full_name(&self) -> &str {
        &self.target_repository_full_name
    }

    fn runner_group_id(&self) -> i64 {
        self.runner_group_id
    }

    fn runner_group_name(&self) -> &str {
        &self.runner_group_name
    }

    fn scale_set_id(&self) -> i64 {
        self.scale_set_id
    }

    fn scale_set_name(&self) -> &str {
        &self.scale_set_name
    }

    fn session_id_for_cleanup(&self) -> &str {
        &self.session_id
    }

    fn begin_delete_attempt(&mut self) -> bool {
        if self.delete_attempted {
            return false;
        }
        self.delete_attempted = true;
        true
    }
}

fn repository_admin(
    transport: &mut FakeTransport,
    intents: &mut Intents,
) -> velnor_runner_github::RepositoryDiscoveryAdmin {
    let evidence = block_on_ready(read_repository_admin_evidence_async(
        transport,
        "ChainArgos",
        "java-monorepo",
        "hostcredential",
    ))
    .expect("repository evidence");
    let token = block_on_ready(issue_repository_discovery_token_async(
        transport,
        evidence,
        "hostcredential",
        intents,
    ))
    .expect("registration token");
    block_on_ready(exchange_repository_discovery_admin_once_async(
        transport, token, intents,
    ))
    .expect("repository discovery admin")
}

fn responses(delete: Result<Exchange, TransportFail>) -> Vec<Result<Exchange, TransportFail>> {
    vec![
        response(200, private_admin_repo()),
        response(201, r#"{"token":"regtoken"}"#),
        response(
            200,
            r#"{"url":"https://pipelinesghubeus9.actions.githubusercontent.com/","token":"admintoken"}"#,
        ),
        response(200, GROUPS),
        response(200, SET),
        delete,
    ]
}

#[test]
fn known_journaled_session_closes_only_on_observed_204() {
    let (mut transport, mut intents) = paired();
    transport
        .0
        .lock()
        .expect("transport lock")
        .responses
        .extend(responses(response(204, "")));
    let admin = repository_admin(&mut transport, &mut intents);
    let binding = binding();
    let route =
        block_on_ready(admin.read_existing_session_cleanup_route_async(&mut transport, &binding))
            .expect("exact existing set route");
    let mut claim = TestClaim::repository("session-1");
    let closed =
        block_on_ready(admin.delete_claimed_session_once_async(&mut transport, route, &mut claim))
            .expect("observed 204");
    assert_eq!(closed, RepositorySessionCleanupOutcome::Closed);

    let state = transport.0.lock().expect("transport lock");
    assert_eq!(state.requests.len(), 6);
    let request = &state.requests[5];
    assert_eq!(request.method, velnor_runner_github::Method::Delete);
    assert_eq!(
        request.path,
        "_apis/runtime/runnerscalesets/3/sessions/session-1"
    );
    assert_eq!(request.query.as_deref(), Some("api-version=6.0-preview"));
    assert!(request.headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("authorization") && value == "Bearer admintoken"
    }));
}

#[test]
fn lost_delete_response_is_uncertain_and_never_retried() {
    let (mut transport, mut intents) = paired();
    transport
        .0
        .lock()
        .expect("transport lock")
        .responses
        .extend(responses(Err(TransportFail::Timeout)));
    let admin = repository_admin(&mut transport, &mut intents);
    let binding = binding();
    let route =
        block_on_ready(admin.read_existing_session_cleanup_route_async(&mut transport, &binding))
            .expect("exact existing set route");
    let mut claim = TestClaim::repository("session-1");
    let result =
        block_on_ready(admin.delete_claimed_session_once_async(&mut transport, route, &mut claim));
    assert_eq!(result, Err(SessionError::Uncertain));

    let state = transport.0.lock().expect("transport lock");
    assert_eq!(state.requests.len(), 6);
    assert_eq!(
        state
            .requests
            .iter()
            .filter(|request| request.method == velnor_runner_github::Method::Delete)
            .count(),
        1
    );
}

#[test]
fn consumed_claim_cannot_dispatch_a_second_delete_after_rebootstrap() {
    let mut claim = TestClaim::repository("session-1");

    let (mut first_transport, mut first_intents) = paired();
    first_transport
        .0
        .lock()
        .expect("transport lock")
        .responses
        .extend(responses(Err(TransportFail::Timeout)));
    let first_admin = repository_admin(&mut first_transport, &mut first_intents);
    let binding = binding();
    let first_route = block_on_ready(
        first_admin.read_existing_session_cleanup_route_async(&mut first_transport, &binding),
    )
    .expect("exact existing set route");
    assert_eq!(
        block_on_ready(first_admin.delete_claimed_session_once_async(
            &mut first_transport,
            first_route,
            &mut claim,
        )),
        Err(SessionError::Uncertain)
    );

    let (mut second_transport, mut second_intents) = paired();
    second_transport
        .0
        .lock()
        .expect("transport lock")
        .responses
        .extend(responses(response(204, "")));
    let second_admin = repository_admin(&mut second_transport, &mut second_intents);
    let second_route = block_on_ready(
        second_admin.read_existing_session_cleanup_route_async(&mut second_transport, &binding),
    )
    .expect("exact existing set route");
    assert!(matches!(
        block_on_ready(second_admin.delete_claimed_session_once_async(
            &mut second_transport,
            second_route,
            &mut claim,
        )),
        Err(SessionError::Wire(WireError::RegistrationRejected))
    ));
    assert_eq!(
        second_transport
            .0
            .lock()
            .expect("transport lock")
            .requests
            .len(),
        5,
        "the reused claim is rejected before DELETE"
    );
}

#[test]
fn non_204_delete_remains_unresolved() {
    let (mut transport, mut intents) = paired();
    transport
        .0
        .lock()
        .expect("transport lock")
        .responses
        .extend(responses(response(404, "")));
    let admin = repository_admin(&mut transport, &mut intents);
    let binding = binding();
    let route =
        block_on_ready(admin.read_existing_session_cleanup_route_async(&mut transport, &binding))
            .expect("exact existing set route");
    let mut claim = TestClaim::repository("session-1");
    let result =
        block_on_ready(admin.delete_claimed_session_once_async(&mut transport, route, &mut claim));
    assert!(matches!(
        result,
        Err(SessionError::Wire(WireError::UnexpectedStatus))
    ));
    assert_eq!(
        transport
            .0
            .lock()
            .expect("transport lock")
            .requests
            .iter()
            .filter(|request| request.method == velnor_runner_github::Method::Delete)
            .count(),
        1
    );
}

#[test]
fn foreign_scope_or_missing_session_id_is_rejected_without_delete() {
    let transport = FakeTransport::default();
    assert!(matches!(
        RepositorySessionCleanupBinding::from_expected_fields(
            RepositorySessionCleanupExpectation {
                destination: "scale-set-controller",
                registration_scope: "organization",
                scope_name: "ChainArgos",
                target_repository_id: 829_618_808,
                target_repository_full_name: "ChainArgos/java-monorepo",
                runner_group_id: 1,
                runner_group_name: "Default",
                scale_set_id: 3,
                scale_set_name: "ubuntu-26.04-scale-set",
            }
        ),
        Err(SessionError::Wire(WireError::RegistrationRejected))
    ));
    assert_eq!(
        transport.0.lock().expect("transport lock").requests.len(),
        0
    );

    for session_id in ["", ".", "..", "session/../other"] {
        let (mut transport, mut intents) = paired();
        transport
            .0
            .lock()
            .expect("transport lock")
            .responses
            .extend(responses(response(204, "")));
        let admin = repository_admin(&mut transport, &mut intents);
        let binding = binding();
        let route = block_on_ready(
            admin.read_existing_session_cleanup_route_async(&mut transport, &binding),
        )
        .expect("exact existing set route");
        let mut claim = TestClaim::repository(session_id);
        let result = block_on_ready(admin.delete_claimed_session_once_async(
            &mut transport,
            route,
            &mut claim,
        ));
        assert!(matches!(
            result,
            Err(SessionError::Wire(WireError::RegistrationRejected))
        ));
        assert_eq!(
            transport.0.lock().expect("transport lock").requests.len(),
            5,
            "unsafe session IDs are rejected before DELETE"
        );
    }
}

#[test]
fn foreign_repository_binding_cannot_delete_a_session() {
    let (mut transport, mut intents) = paired();
    transport
        .0
        .lock()
        .expect("transport lock")
        .responses
        .extend(responses(response(204, "")));
    let admin = repository_admin(&mut transport, &mut intents);
    let expected = binding();
    let route =
        block_on_ready(admin.read_existing_session_cleanup_route_async(&mut transport, &expected))
            .expect("exact existing set route");
    let mut foreign = TestClaim::repository("session-1");
    foreign.scope_name = "Elsewhere/other-repository".to_owned();
    foreign.target_repository_id = 123;
    foreign.target_repository_full_name = "Elsewhere/other-repository".to_owned();
    let result = block_on_ready(admin.delete_claimed_session_once_async(
        &mut transport,
        route,
        &mut foreign,
    ));
    assert!(matches!(
        result,
        Err(SessionError::Wire(WireError::RegistrationRejected))
    ));
    assert_eq!(
        transport.0.lock().expect("transport lock").requests.len(),
        5,
        "a foreign repository is refused before DELETE"
    );
}

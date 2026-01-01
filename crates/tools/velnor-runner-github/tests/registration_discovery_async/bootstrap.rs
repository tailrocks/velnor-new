use super::*;

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "the end-to-end test verifies ordering across the full read-only bootstrap"
)]
fn async_discovery_persists_before_each_post_and_never_exposes_credentials() {
    let (mut transport, mut intents) = paired();
    transport
        .0
        .lock()
        .expect("test transport lock")
        .responses
        .extend([
            response(200, private_admin_repo()),
            response(201, r#"{"token":"regtoken"}"#),
            response(
                200,
                r#"{"url":"https://pipelinesghubeus9.actions.githubusercontent.com/","token":"admintoken"}"#,
            ),
            response(200, r#"{"count":1,"value":[{"id":1,"name":"Default","isDefaultGroup":true}]}"#),
            response(
                200,
                r#"{"count":1,"value":[{"id":3,"name":"ubuntu-24.04-scale-set","labels":[{"name":"velnor","type":"System"},{"name":"ubuntu-24.04-scale-set","type":"System"}],"runnerSetting":{"disableUpdate":true}}]}"#,
            ),
        ]);
    let evidence = block_on_ready(read_repository_admin_evidence_async(
        &mut transport,
        "ChainArgos",
        "java-monorepo",
        "hostcredential",
    ))
    .expect("private admin evidence");
    let token = block_on_ready(issue_repository_discovery_token_async(
        &mut transport,
        evidence,
        "hostcredential",
        &mut intents,
    ))
    .expect("one-shot registration token");
    let admin = block_on_ready(exchange_repository_discovery_admin_once_async(
        &mut transport,
        token,
        &mut intents,
    ))
    .expect("one-shot admin exchange");
    let groups = block_on_ready(admin.list_runner_groups_async(&mut transport))
        .expect("read-only group metadata");
    let scale_set = block_on_ready(admin.get_existing_product_scale_set_async(
        &mut transport,
        1,
        "ubuntu-24.04-scale-set",
    ))
    .expect("read-only exact scale set lookup");

    assert_eq!(groups.len(), 1);
    assert!(matches!(
        scale_set,
        velnor_runner_github::ScaleSetFound::Found(view) if view.id == 3
    ));
    let state = intents.0.lock().expect("test intent lock").clone();
    assert_eq!(
        state.rows,
        vec![
            (
                DiscoveryIntentId::new(1).expect("positive id"),
                DiscoveryCredentialStep::RepositoryRegistrationToken,
                Some(DiscoveryCredentialOutcome::Succeeded),
            ),
            (
                DiscoveryIntentId::new(2).expect("positive id"),
                DiscoveryCredentialStep::ActionsAdminExchange,
                Some(DiscoveryCredentialOutcome::Succeeded),
            ),
        ]
    );
    assert_eq!(state.events, ["intent", "outcome", "intent", "outcome"]);
    let transport_state = transport.0.lock().expect("test transport lock");
    let requests = &transport_state.requests;
    assert_eq!(requests.len(), 5);
    assert_eq!(requests[0].path, "repos/ChainArgos/java-monorepo");
    assert_eq!(
        requests[1].path,
        "/repos/ChainArgos/java-monorepo/actions/runners/registration-token"
    );
    assert_eq!(requests[2].path, "/actions/runner-registration");
    assert_eq!(requests[3].path, "_apis/runtime/runnergroups");
    assert_eq!(requests[4].path, "_apis/runtime/runnerscalesets");
    let debug = format!("{admin:?} {:?}", requests[2]);
    assert!(!debug.contains("admintoken"));
    assert!(!debug.contains("regtoken"));
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "this fixture verifies organization scope from durable intent through the read-only Set route"
)]
fn organization_discovery_uses_scoped_intents_and_exact_internal_group_route() {
    let (mut transport, mut intents) = paired();
    transport
        .0
        .lock()
        .expect("test transport lock")
        .responses
        .extend([
            response(200, private_admin_repo()),
            response(201, r#"{"token":"org-regtoken"}"#),
            response(
                200,
                r#"{"url":"https://pipelinesghubeus9.actions.githubusercontent.com/","token":"org-admintoken"}"#,
            ),
            response(
                200,
                r#"{"count":2,"value":[{"id":11,"name":"trusted-linux","isDefaultGroup":false},{"id":12,"name":"other","isDefaultGroup":true}]}"#,
            ),
            response(
                200,
                r#"{"count":1,"value":[{"id":31,"name":"ubuntu-24.04-scale-set","labels":[{"name":"velnor","type":"System"},{"name":"ubuntu-24.04-scale-set","type":"System"}],"runnerSetting":{"disableUpdate":true}}]}"#,
            ),
        ]);

    let repository = block_on_ready(read_repository_admin_evidence_async(
        &mut transport,
        "ChainArgos",
        "java-monorepo",
        "hostcredential",
    ))
    .expect("private admin evidence");
    let organization = organization_admin_evidence(repository, "ChainArgos")
        .expect("repository owner binds to org scope");
    let token = block_on_ready(issue_organization_discovery_token_async(
        &mut transport,
        organization,
        "hostcredential",
        &mut intents,
    ))
    .expect("org token issuance");
    let admin = block_on_ready(exchange_organization_discovery_admin_once_async(
        &mut transport,
        token,
        &mut intents,
    ))
    .expect("org admin exchange");
    let route = block_on_ready(admin.read_scale_set_route_async(
        &mut transport,
        "trusted-linux",
        "ubuntu-24.04-scale-set",
    ))
    .expect("same-scope route read");

    let ActionsServiceRouteLookup::Found(route) = route else {
        panic!("exact group and set route expected");
    };
    assert_eq!(route.organization(), "ChainArgos");
    assert_eq!(route.runner_group_id(), 11);
    assert_eq!(route.runner_group_name(), "trusted-linux");
    assert_eq!(route.scale_set().id, 31);
    assert_eq!(route.scale_set().name, "ubuntu-24.04-scale-set");

    let state = intents.0.lock().expect("test intent lock").clone();
    assert_eq!(
        state.rows,
        vec![
            (
                DiscoveryIntentId::new(1).expect("positive id"),
                DiscoveryCredentialStep::OrganizationRegistrationToken,
                Some(DiscoveryCredentialOutcome::Succeeded),
            ),
            (
                DiscoveryIntentId::new(2).expect("positive id"),
                DiscoveryCredentialStep::ActionsAdminExchange,
                Some(DiscoveryCredentialOutcome::Succeeded),
            ),
        ]
    );
    assert_eq!(
        state.scopes,
        ["organization:ChainArgos", "organization:ChainArgos"]
    );
    let state = transport.0.lock().expect("test transport lock");
    assert_eq!(state.requests.len(), 5);
    assert_eq!(
        state.requests[1].path,
        "/orgs/ChainArgos/actions/runners/registration-token"
    );
    assert_eq!(state.requests[2].path, "/actions/runner-registration");
    assert!(
        String::from_utf8_lossy(&state.requests[2].body).contains("https://github.com/ChainArgos")
    );
    assert_eq!(state.requests[3].path, "_apis/runtime/runnergroups");
    assert_eq!(state.requests[4].path, "_apis/runtime/runnerscalesets");
    assert_eq!(
        state.requests[4].query.as_deref(),
        Some("api-version=6.0-preview&name=ubuntu-24.04-scale-set&runnerGroupId=11")
    );
    assert!(!format!("{admin:?}").contains("org-admintoken"));
    assert!(!format!("{:?}", state.requests[2]).contains("org-regtoken"));
}

#[test]
fn organization_discovery_rejects_owner_mismatch_before_intent_or_token_post() {
    let (mut transport, intents) = paired();
    transport
        .0
        .lock()
        .expect("test transport lock")
        .responses
        .push_back(response(200, private_admin_repo()));
    let repository = block_on_ready(read_repository_admin_evidence_async(
        &mut transport,
        "ChainArgos",
        "java-monorepo",
        "hostcredential",
    ))
    .expect("private admin evidence");
    assert!(organization_admin_evidence(repository, "UnrelatedOrg").is_err());
    assert_eq!(
        intents.0.lock().expect("test intent lock").rows.as_slice(),
        &[]
    );
    let state = transport.0.lock().expect("test transport lock");
    assert_eq!(state.requests.len(), 1);
    assert_eq!(state.posts_sent, 0);
}

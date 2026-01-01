use super::*;

#[test]
fn duplicate_internal_group_inventory_stops_before_scale_set_lookup() {
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
                r#"{"count":2,"value":[{"id":11,"name":"trusted-linux","isDefaultGroup":false},{"id":12,"name":"trusted-linux","isDefaultGroup":false}]}"#,
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
    assert!(
        block_on_ready(admin.read_scale_set_route_async(
            &mut transport,
            "trusted-linux",
            "ubuntu-24.04-scale-set",
        ))
        .is_err()
    );
    assert_eq!(
        transport
            .0
            .lock()
            .expect("test transport lock")
            .requests
            .len(),
        4
    );
}

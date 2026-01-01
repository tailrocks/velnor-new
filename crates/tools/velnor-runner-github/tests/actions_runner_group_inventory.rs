//! Bounded complete inventory and exact-name runner-group policy lookup tests.

use std::collections::VecDeque;

use velnor_runner_github::{
    Exchange, Method, RunnerGroupAccess, SessionError, SessionRequest, Transport, TransportFail,
    WireError, find_enterprise_runner_group_policy, find_organization_runner_group_policy,
};

const TOKEN: &str = "actions-read-canary";

struct Script {
    replies: VecDeque<Result<Exchange, TransportFail>>,
    seen: Vec<SessionRequest>,
}

impl Script {
    fn responses(bodies: &[String]) -> Self {
        Self {
            replies: bodies
                .iter()
                .map(|body| {
                    Ok(Exchange {
                        status: 200,
                        body: body.as_bytes().to_vec(),
                    })
                })
                .collect(),
            seen: Vec::new(),
        }
    }
}

impl Transport for Script {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.seen.push(request.clone());
        self.replies.pop_front().ok_or(TransportFail::Reset)?
    }
}

#[test]
fn organization_lookup_requires_a_complete_inventory_and_matching_detail() {
    let page_one = group_page(101, 0, 100);
    let page_two = group_page(101, 100, 1);
    let detail = r#"{"id":21,"name":"trusted-linux","visibility":"all","default":false,"inherited":false,"allows_public_repositories":false,"restricted_to_workflows":true,"selected_workflows":["ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/main"],"workflow_restrictions_read_only":false}"#.to_owned();
    let mut script = Script::responses(&[page_one, page_two, detail]);

    let snapshot =
        find_organization_runner_group_policy(&mut script, "ChainArgos", "trusted-linux", TOKEN)
            .expect("complete inventory")
            .expect("one exact group");

    assert_eq!(snapshot.inventory_group_count, 101);
    assert_eq!(snapshot.policy.id, 21);
    assert_eq!(snapshot.policy.name, "trusted-linux");
    assert_eq!(script.seen.len(), 3);
    assert_request(
        &script.seen[0],
        "orgs/ChainArgos/actions/runner-groups",
        Some("per_page=100&page=1"),
    );
    assert_request(
        &script.seen[1],
        "orgs/ChainArgos/actions/runner-groups",
        Some("per_page=100&page=2"),
    );
    assert_request(
        &script.seen[2],
        "orgs/ChainArgos/actions/runner-groups/21",
        None,
    );
    assert!(
        script
            .seen
            .iter()
            .all(|request| request.method == Method::Get)
    );
    assert!(!format!("{snapshot:?}").contains(TOKEN));
}

#[test]
fn duplicate_case_insensitive_names_across_pages_fail_before_detail() {
    let mut first_page: Vec<_> = (0..100)
        .map(|index| {
            serde_json::json!({
                "id": index + 1,
                "name": format!("group-{index}"),
                "visibility": "all"
            })
        })
        .collect();
    first_page[0]["name"] = serde_json::json!("trusted-linux");
    let second_page = serde_json::json!({
        "total_count": 101,
        "runner_groups": [{"id": 101, "name": "Trusted-Linux", "visibility": "all"}]
    })
    .to_string();
    let mut script = Script::responses(&[
        serde_json::json!({"total_count":101,"runner_groups":first_page}).to_string(),
        second_page,
    ]);

    assert_eq!(
        find_organization_runner_group_policy(&mut script, "ChainArgos", "trusted-linux", TOKEN),
        Err(SessionError::Wire(WireError::Malformed))
    );
    assert_eq!(script.seen.len(), 2);
}

#[test]
fn detail_change_after_inventory_is_rejected() {
    let inventory = serde_json::json!({
        "total_count": 1,
        "runner_groups": [{
            "id": 7,
            "name": "trusted-linux",
            "visibility": "selected",
            "restricted_to_workflows": true
        }]
    })
    .to_string();
    let detail =
        r#"{"id":7,"name":"trusted-linux","visibility":"all","restricted_to_workflows":true}"#
            .to_owned();
    let mut script = Script::responses(&[inventory, detail]);

    assert_eq!(
        find_organization_runner_group_policy(&mut script, "ChainArgos", "trusted-linux", TOKEN),
        Err(SessionError::Wire(WireError::Malformed))
    );
    assert_eq!(script.seen.len(), 2);
}

#[test]
fn missing_exact_name_and_invalid_name_do_not_issue_extra_requests() {
    let inventory = serde_json::json!({
        "total_count": 1,
        "runner_groups": [{"id": 7, "name": "other", "visibility": "all"}]
    })
    .to_string();
    let mut script = Script::responses(&[inventory]);
    assert_eq!(
        find_organization_runner_group_policy(&mut script, "ChainArgos", "trusted", TOKEN),
        Ok(None)
    );
    assert_eq!(script.seen.len(), 1);

    assert_eq!(
        find_organization_runner_group_policy(&mut script, "ChainArgos", "bad\nname", TOKEN),
        Err(SessionError::Wire(WireError::RegistrationRejected))
    );
    assert_eq!(script.seen.len(), 1);
}

#[test]
fn enterprise_inventory_preserves_scope_and_reads_selected_organizations() {
    let inventory = serde_json::json!({
        "total_count": 1,
        "runner_groups": [{"id": 12, "name": "trusted-enterprise", "visibility": "selected"}]
    })
    .to_string();
    let detail = r#"{"id":12,"name":"trusted-enterprise","visibility":"selected","allows_public_repositories":false,"restricted_to_workflows":true,"selected_workflows":["ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/main"]}"#.to_owned();
    let organizations =
        r#"{"total_count":1,"organizations":[{"id":161335,"login":"ChainArgos"}]}"#.to_owned();
    let mut script = Script::responses(&[inventory, detail, organizations]);

    let snapshot = find_enterprise_runner_group_policy(
        &mut script,
        "enterprise-slug",
        "trusted-enterprise",
        TOKEN,
    )
    .expect("complete inventory")
    .expect("one exact group");
    assert_eq!(snapshot.inventory_group_count, 1);
    assert_eq!(snapshot.policy.id, 12);
    assert_eq!(
        snapshot.policy.access,
        RunnerGroupAccess::SelectedOrganizations(vec![
            velnor_runner_github::SelectedOrganization {
                id: 161_335,
                login: "ChainArgos".to_owned(),
            }
        ])
    );
    assert_eq!(script.seen.len(), 3);
    assert_request(
        &script.seen[0],
        "enterprises/enterprise-slug/actions/runner-groups",
        Some("per_page=100&page=1"),
    );
    assert_request(
        &script.seen[2],
        "enterprises/enterprise-slug/actions/runner-groups/12/organizations",
        Some("per_page=100&page=1"),
    );
}

fn group_page(total: usize, start: usize, length: usize) -> String {
    let runner_groups: Vec<_> = (start..start + length)
        .map(|index| {
            serde_json::json!({
                "id": index + 1,
                "name": if index == 20 { "trusted-linux".to_owned() } else { format!("group-{index}") },
                "visibility": "all",
                "default": false,
                "inherited": false,
                "allows_public_repositories": false,
                "restricted_to_workflows": true,
                "selected_workflows": ["ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/main"],
                "workflow_restrictions_read_only": false
            })
        })
        .collect();
    serde_json::json!({ "total_count": total, "runner_groups": runner_groups }).to_string()
}

fn assert_request(request: &SessionRequest, path: &str, query: Option<&str>) {
    assert_eq!(request.path, path);
    assert_eq!(request.query.as_deref(), query);
    assert_eq!(request.method, Method::Get);
    assert_eq!(request.body, Vec::<u8>::new());
    assert_eq!(
        request
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("Authorization"))
            .map(|(_, value)| value.as_str()),
        Some("Bearer actions-read-canary")
    );
}

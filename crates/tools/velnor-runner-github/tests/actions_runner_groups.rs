//! Bounded REST runner-group policy-read tests.

use std::collections::VecDeque;

use velnor_runner_github::{
    ActionsRunnerGroupPolicy, Exchange, Method, RunnerGroupAccess, RunnerGroupScope,
    SelectedOrganization, SelectedRepository, SessionError, SessionRequest, Transport,
    TransportFail, WireError, get_enterprise_runner_group_policy,
    get_organization_runner_group_policy,
};

const TOKEN: &str = "actions-read-canary";

struct Script {
    replies: VecDeque<Result<Exchange, TransportFail>>,
    seen: Vec<SessionRequest>,
}

impl Script {
    fn responses(statuses_and_bodies: &[(u16, String)]) -> Self {
        Self {
            replies: statuses_and_bodies
                .iter()
                .map(|(status, body)| {
                    Ok(Exchange {
                        status: *status,
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
fn organization_policy_reads_group_and_complete_selected_repository_page() {
    let mut script = Script::responses(&[
        (
            200,
            r#"{"id":7,"name":"trusted-linux","visibility":"selected","default":false,"inherited":false,"allows_public_repositories":false,"restricted_to_workflows":true,"selected_workflows":["ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/main"],"workflow_restrictions_read_only":false}"#.to_owned(),
        ),
        (
            200,
            r#"{"total_count":2,"repositories":[{"id":829618808,"name":"java-monorepo","full_name":"ChainArgos/java-monorepo","private":true},{"id":88,"name":"runner-config","full_name":"Velnor/runner-config","private":true}]}"#.to_owned(),
        ),
    ]);

    let policy = get_organization_runner_group_policy(&mut script, "ChainArgos", 7, TOKEN)
        .expect("organization group policy");

    assert_eq!(
        policy,
        ActionsRunnerGroupPolicy {
            scope: RunnerGroupScope::Organization("ChainArgos".to_owned()),
            id: 7,
            name: "trusted-linux".to_owned(),
            visibility: "selected".to_owned(),
            is_default: Some(false),
            inherited: Some(false),
            allows_public_repositories: Some(false),
            restricted_to_workflows: Some(true),
            selected_workflows: Some(vec![
                "ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/main".to_owned()
            ]),
            workflow_restrictions_read_only: Some(false),
            access: RunnerGroupAccess::SelectedRepositories(vec![
                SelectedRepository {
                    id: 829_618_808,
                    name: "java-monorepo".to_owned(),
                    full_name: "ChainArgos/java-monorepo".to_owned(),
                    private: Some(true),
                },
                SelectedRepository {
                    id: 88,
                    name: "runner-config".to_owned(),
                    full_name: "Velnor/runner-config".to_owned(),
                    private: Some(true),
                },
            ]),
        }
    );
    assert_eq!(script.seen.len(), 2);
    assert_request(
        &script.seen[0],
        "orgs/ChainArgos/actions/runner-groups/7",
        None,
    );
    assert_request(
        &script.seen[1],
        "orgs/ChainArgos/actions/runner-groups/7/repositories",
        Some("per_page=100&page=1"),
    );
}

#[test]
fn enterprise_selected_access_reads_organizations_and_preserves_scope() {
    let mut script = Script::responses(&[
        (
            200,
            r#"{"id":12,"name":"enterprise-trusted","visibility":"selected","allows_public_repositories":false,"restricted_to_workflows":true,"selected_workflows":["ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/main"]}"#.to_owned(),
        ),
        (
            200,
            r#"{"total_count":1,"organizations":[{"id":161335,"login":"ChainArgos"}]}"#.to_owned(),
        ),
    ]);

    let policy = get_enterprise_runner_group_policy(&mut script, "enterprise-slug", 12, TOKEN)
        .expect("enterprise group policy");

    assert_eq!(
        policy.scope,
        RunnerGroupScope::Enterprise("enterprise-slug".to_owned())
    );
    assert_eq!(
        policy.access,
        RunnerGroupAccess::SelectedOrganizations(vec![SelectedOrganization {
            id: 161_335,
            login: "ChainArgos".to_owned(),
        }])
    );
    assert_eq!(script.seen.len(), 2);
    assert_request(
        &script.seen[0],
        "enterprises/enterprise-slug/actions/runner-groups/12",
        None,
    );
    assert_request(
        &script.seen[1],
        "enterprises/enterprise-slug/actions/runner-groups/12/organizations",
        Some("per_page=100&page=1"),
    );
}

#[test]
fn pagination_reconciles_all_pages_and_rejects_duplicate_identity() {
    let group = r#"{"id":7,"name":"trusted","visibility":"selected"}"#.to_owned();
    let first_page = json_page(101, 0, 100);
    let final_page = json_page(101, 100, 1);
    let mut complete =
        Script::responses(&[(200, group.clone()), (200, first_page), (200, final_page)]);
    let policy = get_organization_runner_group_policy(&mut complete, "ChainArgos", 7, TOKEN)
        .expect("all pages");
    let RunnerGroupAccess::SelectedRepositories(repositories) = policy.access else {
        panic!("selected repository page shape");
    };
    assert_eq!(repositories.len(), 101);
    assert_eq!(complete.seen.len(), 3);
    assert_eq!(
        complete.seen[1].query.as_deref(),
        Some("per_page=100&page=1")
    );
    assert_eq!(
        complete.seen[2].query.as_deref(),
        Some("per_page=100&page=2")
    );

    let duplicate_second = serde_json::json!({
        "total_count": 101,
        "repositories": [{
            "id": 100,
            "name": "repo-100",
            "full_name": "Org/repo-100",
            "private": true
        }]
    })
    .to_string();
    let mut duplicate = Script::responses(&[
        (200, group),
        (200, json_page(101, 0, 100)),
        (200, duplicate_second),
    ]);
    assert_eq!(
        get_organization_runner_group_policy(&mut duplicate, "ChainArgos", 7, TOKEN),
        Err(SessionError::Wire(WireError::Malformed))
    );
    assert_eq!(duplicate.seen.len(), 3);

    let inconsistent_total = serde_json::json!({
        "total_count": 102,
        "repositories": [
            {"id": 101, "name": "repo-100", "full_name": "Org/repo-100"},
            {"id": 102, "name": "repo-101", "full_name": "Org/repo-101"}
        ]
    })
    .to_string();
    let mut changed_total = Script::responses(&[
        (
            200,
            r#"{"id":7,"name":"trusted","visibility":"selected"}"#.to_owned(),
        ),
        (200, json_page(101, 0, 100)),
        (200, inconsistent_total),
    ]);
    assert_eq!(
        get_organization_runner_group_policy(&mut changed_total, "ChainArgos", 7, TOKEN),
        Err(SessionError::Wire(WireError::Malformed))
    );
    assert_eq!(changed_total.seen.len(), 3);
}

#[test]
fn all_visibility_is_explicit_and_does_not_fetch_a_selected_list() {
    let mut script = Script::responses(&[(
        200,
        r#"{"id":1,"name":"Default","visibility":"all"}"#.to_owned(),
    )]);
    let policy =
        get_organization_runner_group_policy(&mut script, "org", 1, TOKEN).expect("all group");
    assert_eq!(policy.access, RunnerGroupAccess::All);
    assert_eq!(policy.allows_public_repositories, None);
    assert_eq!(script.seen.len(), 1);
}

#[test]
fn invalid_inputs_and_incomplete_or_unsupported_responses_fail_closed() {
    let mut script = Script::responses(&[]);
    assert_eq!(
        get_organization_runner_group_policy(&mut script, "../org", 1, TOKEN),
        Err(SessionError::Wire(WireError::RegistrationRejected))
    );
    assert_eq!(
        get_organization_runner_group_policy(&mut script, "org", 0, TOKEN),
        Err(SessionError::Wire(WireError::RegistrationRejected))
    );
    assert_eq!(
        get_organization_runner_group_policy(&mut script, "org", 1, "bad token"),
        Err(SessionError::Wire(WireError::RegistrationRejected))
    );
    assert_eq!(script.seen, Vec::<SessionRequest>::new());

    let mut unknown_visibility = Script::responses(&[(
        200,
        r#"{"id":1,"name":"Default","visibility":"unknown"}"#.to_owned(),
    )]);
    assert_eq!(
        get_organization_runner_group_policy(&mut unknown_visibility, "org", 1, TOKEN),
        Err(SessionError::Wire(WireError::Malformed))
    );
    assert_eq!(unknown_visibility.seen.len(), 1);

    let mut mismatched_id = Script::responses(&[(
        200,
        r#"{"id":2,"name":"Default","visibility":"all"}"#.to_owned(),
    )]);
    assert_eq!(
        get_organization_runner_group_policy(&mut mismatched_id, "org", 1, TOKEN),
        Err(SessionError::Wire(WireError::Malformed))
    );

    let mut missing_item = Script::responses(&[
        (
            200,
            r#"{"id":7,"name":"trusted","visibility":"selected"}"#.to_owned(),
        ),
        (
            200,
            r#"{"total_count":2,"repositories":[{"id":8,"name":"repo","full_name":"Org/repo"}]}"#
                .to_owned(),
        ),
    ]);
    assert_eq!(
        get_organization_runner_group_policy(&mut missing_item, "org", 7, TOKEN),
        Err(SessionError::Wire(WireError::Malformed))
    );
    assert_eq!(missing_item.seen.len(), 2);

    let oversized = serde_json::json!({"total_count": 3201, "repositories": []}).to_string();
    let mut too_many = Script::responses(&[
        (
            200,
            r#"{"id":7,"name":"trusted","visibility":"selected"}"#.to_owned(),
        ),
        (200, oversized),
    ]);
    assert_eq!(
        get_organization_runner_group_policy(&mut too_many, "org", 7, TOKEN),
        Err(SessionError::Wire(WireError::Malformed))
    );
    assert_eq!(too_many.seen.len(), 2);

    let large_body = format!(
        r#"{{"id":7,"name":"trusted","visibility":"all","padding":"{}"}}"#,
        "x".repeat(1_048_577)
    );
    let mut oversized_body = Script::responses(&[(200, large_body)]);
    assert_eq!(
        get_organization_runner_group_policy(&mut oversized_body, "org", 7, TOKEN),
        Err(SessionError::Wire(WireError::Malformed))
    );
    assert_eq!(oversized_body.seen.len(), 1);
}

#[test]
fn inaccessible_group_is_an_error_without_create_fallback() {
    let mut script = Script::responses(&[(404, "{}".to_owned())]);
    assert_eq!(
        get_organization_runner_group_policy(&mut script, "org", 9, TOKEN),
        Err(SessionError::Wire(WireError::UnexpectedStatus))
    );
    assert_eq!(script.seen.len(), 1);
    assert_eq!(script.seen[0].method, Method::Get);
}

fn json_page(total: usize, start: usize, length: usize) -> String {
    let repositories: Vec<_> = (start..start + length)
        .map(|index| {
            serde_json::json!({
                "id": index + 1,
                "name": format!("repo-{index}"),
                "full_name": format!("Org/repo-{index}"),
                "private": true
            })
        })
        .collect();
    serde_json::json!({
        "total_count": total,
        "repositories": repositories
    })
    .to_string()
}

fn assert_request(request: &SessionRequest, path: &str, query: Option<&str>) {
    assert_eq!(request.method, Method::Get);
    assert_eq!(request.path, path);
    assert_eq!(request.query.as_deref(), query);
    assert_eq!(request.body, Vec::<u8>::new());
    assert_eq!(
        header(request, "Accept"),
        Some("application/vnd.github+json")
    );
    assert_eq!(header(request, "X-GitHub-Api-Version"), Some("2026-03-10"));
    assert_eq!(
        header(request, "Authorization"),
        Some("Bearer actions-read-canary")
    );
    assert!(!format!("{request:?}").contains(TOKEN));
}

fn header<'a>(request: &'a SessionRequest, name: &str) -> Option<&'a str> {
    request
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

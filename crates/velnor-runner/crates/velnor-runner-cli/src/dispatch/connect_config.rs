use velnor_runner_host::{PullRequestRule, RepositoryConnection, WorkflowEvent};

use super::connect::ConnectRequest;

pub(super) fn sample_config(
    request: &ConnectRequest<'_>,
    connection: &RepositoryConnection,
) -> String {
    let connection_id = toml_basic_string(connection.connection_id.as_str());
    let repository = toml_basic_string(&connection.repository);
    let credential_ref = toml_basic_string(&connection.credential_ref.to_string());
    let scale_set_name = toml_basic_string(connection.scale_set.name());
    let labels = toml_string_array(connection.scale_set.labels().iter().map(String::as_str));
    let branches = toml_string_array(connection.source_policy.branches.iter().map(String::as_str));
    let tag_prefixes = toml_string_array(
        connection
            .source_policy
            .tag_prefixes
            .iter()
            .map(String::as_str),
    );
    let events = toml_string_array(
        connection
            .source_policy
            .events
            .iter()
            .map(|event| event_name(*event)),
    );
    let source_policy_mode = match &connection.source_policy.mode {
        velnor_runner_host::SourcePolicyMode::Repository => "repository",
        velnor_runner_host::SourcePolicyMode::BranchRestricted => "branch_restricted",
    };
    let source_policy_mode = toml_basic_string(source_policy_mode);
    let pull_request_rules = toml_pull_request_rules(&connection.source_policy.pull_request_rules);
    let docker_context = toml_basic_string(request.docker_context.unwrap_or("orbstack"));
    let endpoint = toml_basic_string(request.endpoint);
    let platform = toml_basic_string(request.platform);
    format!(
        "schema = 2\n[[connections]]\nconnection_id = {connection_id}\nrepository_id = {}\nrepository = {repository}\ncredential_ref = {credential_ref}\n[connections.scale_set]\nname = {scale_set_name}\nlabels = {labels}\n[connections.source_policy]\nmode = {source_policy_mode}\nbranches = {branches}\ntag_prefixes = {tag_prefixes}\nevents = {events}\npull_request_rules = {pull_request_rules}\n[host]\nmax_jobs = {}\n[host.resources]\nrunner_cpu_millicores = {}\nrunner_memory_bytes = {}\ndind_cpu_millicores = {}\ndind_memory_bytes = {}\noperational_memory_reserve_bytes = {}\n[docker]\ncontext = {docker_context}\nplatform = {platform}\nendpoint = {endpoint}\n",
        connection.repository_id,
        request.max_jobs.unwrap_or(1),
        request.runner_cpu_millicores,
        request.runner_memory_bytes,
        request.dind_cpu_millicores,
        request.dind_memory_bytes,
        request.operational_memory_reserve_bytes,
    )
}

fn event_name(event: WorkflowEvent) -> &'static str {
    match event {
        WorkflowEvent::PullRequest => "pull_request",
        WorkflowEvent::Push => "push",
        WorkflowEvent::MergeGroup => "merge_group",
        WorkflowEvent::Schedule => "schedule",
        WorkflowEvent::WorkflowDispatch => "workflow_dispatch",
        WorkflowEvent::Release => "release",
    }
}

fn toml_string_array<'a>(values: impl Iterator<Item = &'a str>) -> String {
    let values = values.map(toml_basic_string).collect::<Vec<_>>().join(", ");
    format!("[{values}]")
}

fn toml_pull_request_rules(rules: &[PullRequestRule]) -> String {
    let rules = rules
        .iter()
        .map(|rule| {
            let number = rule
                .number
                .map_or_else(String::new, |number| format!("number = {number}, "));
            let base_branch = toml_basic_string(&rule.base_branch);
            let head_branch = toml_basic_string(&rule.head_branch);
            format!(
                "{{ {number}base_repository_id = {}, base_branch = {base_branch}, head_repository_id = {}, head_branch = {head_branch} }}",
                rule.base_repository_id, rule.head_repository_id
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{rules}]")
}

fn toml_basic_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\u{8}' => escaped.push_str("\\b"),
            '\t' => escaped.push_str("\\t"),
            '\n' => escaped.push_str("\\n"),
            '\u{c}' => escaped.push_str("\\f"),
            '\r' => escaped.push_str("\\r"),
            ch if ch.is_control() => {
                const HEX: &[u8; 16] = b"0123456789ABCDEF";
                let codepoint = u32::from(ch);
                escaped.push_str("\\u");
                for shift in (0..4).rev() {
                    let digit = ((codepoint >> (shift * 4)) & 0xf) as usize;
                    escaped.push(char::from(HEX[digit]));
                }
            }
            ch => escaped.push(ch),
        }
    }
    format!("\"{escaped}\"")
}

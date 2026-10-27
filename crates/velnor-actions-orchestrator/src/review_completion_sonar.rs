// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::super::{feedback::FeedbackRecords, github::TapMergeGitHub};
use super::time;
use crate::{OrchestratorError, internal::internal};
use serde_json::{Value, json};
use std::collections::BTreeSet;

const APP_ID: u64 = 12_526;
const APP_OWNER: u64 = 545_988;
const BOT_ID: u64 = 39_514_782;
const APP: &str = "sonarqubecloud";
const OWNER: &str = "SonarSource";
const EDITOR: &str = "query($id:ID!){node(id:$id){... on IssueComment{fullDatabaseId body updatedAt lastEditedAt author{__typename ... on Bot{databaseId}} editor{__typename ... on Bot{databaseId}}}}}";
const COMMENT_TEMPLATE: &str = "## [![Quality Gate Passed](https://sonarsource.github.io/sonarcloud-github-static-resources/v2/checks/QualityGateBadge/qg-passed-20px.png 'Quality Gate Passed')](https://sonarcloud.io/dashboard?id={project}&pullRequest={number}) **Quality Gate passed**  \nIssues  \n![](https://sonarsource.github.io/sonarcloud-github-static-resources/v2/common/passed-16px.png '') [0 New issues](https://sonarcloud.io/project/issues?id={project}&pullRequest={number}&issueStatuses=OPEN,CONFIRMED&sinceLeakPeriod=true)  \n![](https://sonarsource.github.io/sonarcloud-github-static-resources/v2/common/accepted-16px.png '') [0 Accepted issues](https://sonarcloud.io/project/issues?id={project}&pullRequest={number}&issueStatuses=ACCEPTED)\n\nMeasures  \n![](https://sonarsource.github.io/sonarcloud-github-static-resources/v2/common/passed-16px.png '') [0 Security Hotspots](https://sonarcloud.io/project/security_hotspots?id={project}&pullRequest={number}&issueStatuses=OPEN,CONFIRMED&sinceLeakPeriod=true)  \n![](https://sonarsource.github.io/sonarcloud-github-static-resources/v2/common/passed-16px.png '') [{coverage}% Coverage on New Code](https://sonarcloud.io/component_measures?id={project}&pullRequest={number}&metric=new_coverage&view=list)  \n![](https://sonarsource.github.io/sonarcloud-github-static-resources/v2/common/passed-16px.png '') [{duplication}% Duplication on New Code](https://sonarcloud.io/component_measures?id={project}&pullRequest={number}&metric=new_duplicated_lines_density&view=list)  \n  \n<!-- sqra-placement-anchor -->\n[See analysis details on SonarQube Cloud](https://sonarcloud.io/dashboard?id={project}&pullRequest={number})\n\n";
const CHECK_TEMPLATE: &str = "Issues  \n![](https://sonarsource.github.io/sonarcloud-github-static-resources/v2/common/passed.svg '') [0 New issues](https://sonarcloud.io/project/issues?id={project}&pullRequest={number}&issueStatuses=OPEN,CONFIRMED&sinceLeakPeriod=true)  \n![](https://sonarsource.github.io/sonarcloud-github-static-resources/v2/common/accepted.svg '') [0 Accepted issues](https://sonarcloud.io/project/issues?id={project}&pullRequest={number}&issueStatuses=ACCEPTED)\n\nMeasures  \n![](https://sonarsource.github.io/sonarcloud-github-static-resources/v2/common/passed.svg '') [0 Security Hotspots](https://sonarcloud.io/project/security_hotspots?id={project}&pullRequest={number}&issueStatuses=OPEN,CONFIRMED&sinceLeakPeriod=true)  \n![](https://sonarsource.github.io/sonarcloud-github-static-resources/v2/common/passed.svg '') [{coverage}% Coverage on New Code](https://sonarcloud.io/component_measures?id={project}&pullRequest={number}&metric=new_coverage&view=list)  \n![](https://sonarsource.github.io/sonarcloud-github-static-resources/v2/common/passed.svg '') [{duplication}% Duplication on New Code](https://sonarcloud.io/component_measures?id={project}&pullRequest={number}&metric=new_duplicated_lines_density&view=list)  \n  \n[See analysis details on SonarQube Cloud](https://sonarcloud.io/dashboard?id={project}&pullRequest={number})\n\n";

pub(super) fn classify(
    api: &TapMergeGitHub,
    records: &FeedbackRecords,
    summary: &Value,
    head: &str,
    terminal: (time::Timestamp, time::Timestamp),
    restart: Option<(&str, &str)>,
    location: (&str, u64, u64, Option<u64>),
) -> Result<(bool, Vec<Value>), OrchestratorError> {
    if let Some((role, stamp)) = restart {
        let stamp = time::parse(stamp)?;
        if (role == "code" && terminal.0 <= stamp) || (role == "security" && terminal.1 <= stamp) {
            return Ok((false, Vec::new()));
        }
    }
    let mut ids = BTreeSet::new();
    let mut metadata = Vec::new();
    for comment in &records.issue_comments {
        let id = comment["id"]
            .as_u64()
            .filter(|id| *id > 0)
            .ok_or_else(|| internal("review_provider_comment_id"))?;
        if !ids.insert(id) {
            return Err(internal("review_provider_comment_id"));
        }
        if comment["id"] == summary["id"] {
            continue;
        }
        let text = comment["body"]
            .as_str()
            .ok_or_else(|| internal("review_provider_unknown_feedback"))?;
        if let Some(role) = super::request_role(text) {
            let stamp = time::parse(
                comment["updated_at"]
                    .as_str()
                    .ok_or_else(|| internal("review_request_time"))?,
            )?;
            if (role == "code" && terminal.0 <= stamp)
                || (role == "security" && terminal.1 <= stamp)
            {
                return Ok((false, metadata));
            }
            continue;
        }
        if let Some(evidence) = authenticate(api, comment, head, location)? {
            metadata.push(evidence);
        } else {
            return Err(internal("review_provider_unknown_feedback"));
        }
    }
    Ok((true, metadata))
}

fn authenticate(
    api: &TapMergeGitHub,
    comment: &Value,
    head: &str,
    location: (&str, u64, u64, Option<u64>),
) -> Result<Option<Value>, OrchestratorError> {
    if !candidate(comment) {
        return Ok(None);
    }
    let app: Value = api.get("apps/sonarqubecloud")?;
    if app["id"].as_u64() != Some(APP_ID)
        || app["slug"].as_str() != Some(APP)
        || app["owner"]["id"].as_u64() != Some(APP_OWNER)
        || app["owner"]["login"].as_str() != Some(OWNER)
        || app["owner"]["type"]
            .as_str()
            .is_some_and(|kind| kind != "Organization")
    {
        return Err(internal("review_sonar_app"));
    }
    if comment["id"].as_u64().is_none_or(|id| id == 0)
        || comment.pointer("/user/id").and_then(Value::as_u64) != Some(BOT_ID)
        || comment.pointer("/user/type").and_then(Value::as_str) != Some("Bot")
        || comment
            .pointer("/performed_via_github_app/id")
            .and_then(Value::as_u64)
            != Some(APP_ID)
        || comment
            .pointer("/performed_via_github_app/slug")
            .and_then(Value::as_str)
            != Some(APP)
        || comment
            .pointer("/performed_via_github_app/owner/id")
            .and_then(Value::as_u64)
            != Some(APP_OWNER)
    {
        return Err(internal("review_sonar_author"));
    }
    let node_id = comment["node_id"]
        .as_str()
        .ok_or_else(|| internal("review_sonar_node"))?;
    let editor: Value = api.graphql(EDITOR, &[("id", node_id.to_owned())])?;
    authenticate_editor(comment, &editor)?;
    let body = comment["body"]
        .as_str()
        .ok_or_else(|| internal("review_sonar_body"))?;
    let checks = super::super::super::observation::check_runs(api, location.0, head)?;
    let matches = checks
        .iter()
        .filter(|check| {
            check["name"].as_str() == Some("SonarCloud Code Analysis")
                && check.pointer("/app/id").and_then(Value::as_u64) == Some(APP_ID)
        })
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(internal("review_sonar_check_ambiguous"));
    }
    let check = matches[0];
    let project = project_key(check["details_url"].as_str(), location.1)?;
    if !valid_check(check, head, location, &project)
        || !valid_report(body, true, &project, location.1)
        || !valid_report(
            check
                .pointer("/output/summary")
                .and_then(Value::as_str)
                .ok_or_else(|| internal("review_sonar_check_summary"))?,
            false,
            &project,
            location.1,
        )
    {
        return Err(internal("review_sonar_evidence"));
    }
    Ok(Some(json!({"app":app,"editor":editor,"check":check,"fork_head_repository_id":location.3})))
}

fn candidate(comment: &Value) -> bool {
    comment
        .pointer("/performed_via_github_app/id")
        .and_then(Value::as_u64)
        == Some(APP_ID)
        || comment
            .pointer("/performed_via_github_app/slug")
            .and_then(Value::as_str)
            == Some(APP)
        || comment.pointer("/user/id").and_then(Value::as_u64) == Some(BOT_ID)
}

fn authenticate_editor(comment: &Value, response: &Value) -> Result<(), OrchestratorError> {
    let node = &response["data"]["node"];
    if response.get("errors").is_some()
        || super::full_database_id(node).is_none_or(|id| comment["id"].as_u64() != Some(id))
        || node["body"] != comment["body"]
        || node["updatedAt"] != comment["updated_at"]
        || node.pointer("/author/databaseId").and_then(Value::as_u64) != Some(BOT_ID)
        || node.pointer("/author/__typename").and_then(Value::as_str) != Some("Bot")
        || node.get("lastEditedAt").is_none()
        || node.get("editor").is_none()
    {
        return Err(internal("review_sonar_editor"));
    }
    let created = time::parse(
        comment["created_at"]
            .as_str()
            .ok_or_else(|| internal("review_sonar_editor"))?,
    )?;
    let updated = time::parse(
        comment["updated_at"]
            .as_str()
            .ok_or_else(|| internal("review_sonar_editor"))?,
    )?;
    if updated < created {
        return Err(internal("review_sonar_editor"));
    }
    if node["lastEditedAt"].is_null() && node["editor"].is_null() {
        if comment["created_at"] != comment["updated_at"] {
            return Err(internal("review_sonar_editor"));
        }
        return Ok(());
    }
    let edited = time::parse(
        node["lastEditedAt"]
            .as_str()
            .ok_or_else(|| internal("review_sonar_editor"))?,
    )?;
    if edited < created
        || edited > updated
        || node.pointer("/editor/databaseId").and_then(Value::as_u64) != Some(BOT_ID)
        || node.pointer("/editor/__typename").and_then(Value::as_str) != Some("Bot")
    {
        return Err(internal("review_sonar_editor"));
    }
    Ok(())
}

fn project_key(details_url: Option<&str>, number: u64) -> Result<String, OrchestratorError> {
    let prefix = "https://sonarcloud.io/dashboard?id=";
    let suffix = format!("&pullRequest={number}");
    let project = details_url
        .and_then(|url| url.strip_prefix(prefix))
        .and_then(|url| url.strip_suffix(&suffix))
        .filter(|project| valid_project_key(project))
        .ok_or_else(|| internal("review_sonar_project"))?;
    Ok(project.to_owned())
}

fn valid_project_key(project: &str) -> bool {
    let bytes = project.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index].is_ascii_alphanumeric() || b"_.:-".contains(&bytes[index]) {
            index += 1;
        } else if bytes[index] == b'%'
            && bytes.get(index + 1).is_some_and(u8::is_ascii_hexdigit)
            && bytes.get(index + 2).is_some_and(u8::is_ascii_hexdigit)
        {
            index += 3;
        } else {
            return false;
        }
    }
    !bytes.is_empty()
}

fn valid_check(check: &Value, head: &str, location: (&str, u64, u64, Option<u64>), project: &str) -> bool {
    let link = format!(
        "https://sonarcloud.io/dashboard?id={project}&pullRequest={}",
        location.1
    );
    check["head_sha"].as_str() == Some(head)
        && check["status"].as_str() == Some("completed")
        && check["conclusion"].as_str() == Some("success")
        && check["output"]["title"].as_str() == Some("Quality Gate passed")
        && check["output"]["annotations_count"].as_u64() == Some(0)
        && check["details_url"].as_str() == Some(link.as_str())
        && check.pointer("/app/id").and_then(Value::as_u64) == Some(APP_ID)
        && check.pointer("/app/slug").and_then(Value::as_str) == Some(APP)
        && check.pointer("/app/owner/id").and_then(Value::as_u64) == Some(APP_OWNER)
        && check["pull_requests"].as_array().is_some_and(|pulls| {
            (pulls.is_empty() && location.3.is_some())
                || pulls
                    .iter()
                    .filter(|pull| {
                        pull["number"].as_u64() == Some(location.1)
                            && pull.pointer("/head/sha").and_then(Value::as_str) == Some(head)
                            && pull.pointer("/base/repo/id").and_then(Value::as_u64)
                                == Some(location.2)
                    })
                    .count()
                    == 1
        })
}

fn valid_report(report: &str, comment: bool, project: &str, number: u64) -> bool {
    let coverage_index = if comment { 7 } else { 6 };
    let lines = report.lines().collect::<Vec<_>>();
    let Some(coverage_line) = lines.get(coverage_index) else {
        return false;
    };
    let Some(duplication_line) = lines.get(coverage_index + 1) else {
        return false;
    };
    let coverage = metric(
        coverage_line,
        "Coverage on New Code",
        "new_coverage",
        project,
        number,
    );
    let duplication = metric(
        duplication_line,
        "Duplication on New Code",
        "new_duplicated_lines_density",
        project,
        number,
    );
    let (Some(coverage), Some(duplication)) = (coverage, duplication) else {
        return false;
    };
    let template = if comment {
        COMMENT_TEMPLATE
    } else {
        CHECK_TEMPLATE
    };
    let expected = template
        .replace("{project}", project)
        .replace("{number}", &number.to_string())
        .replace("{coverage}", coverage)
        .replace("{duplication}", duplication);
    report == expected
}

fn metric<'a>(
    line: &'a str,
    label: &str,
    metric: &str,
    project: &str,
    number: u64,
) -> Option<&'a str> {
    let prefix = "![](https://sonarsource.github.io/sonarcloud-github-static-resources/v2/common/";
    let (icon, rest) = line.strip_prefix(prefix)?.split_once(" '') [")?;
    if !matches!(icon, "passed-16px.png" | "passed.svg") {
        return None;
    }
    let (value, _) = rest.split_once(label)?;
    let value = value.strip_suffix("% ")?;
    let expected = format!(
        "](https://sonarcloud.io/component_measures?id={project}&pullRequest={number}&metric={metric}&view=list)  "
    );
    if rest.strip_prefix(&format!("{value}% {label}"))? != expected {
        return None;
    }
    valid_percent(value).then_some(value)
}

fn valid_percent(value: &str) -> bool {
    let Some((whole, fraction)) = value.split_once('.') else {
        return value.parse::<u8>().is_ok_and(|number| number <= 100);
    };
    if whole.is_empty()
        || fraction.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return false;
    }
    whole.parse::<u8>().is_ok_and(|number| {
        number < 100 || number == 100 && fraction.bytes().all(|byte| byte == b'0')
    })
}

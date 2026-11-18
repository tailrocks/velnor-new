// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_github::{
    Exchange, Method, RegistrationTokenCall, RunnerGroup, SessionRequest, Transport, TransportFail,
    registration_token,
};

use super::RegistrationTarget;
use crate::config::{
    ConnectionId, KeychainReference, RepositoryConnection, ScaleSetSelector, SourcePolicy,
    SourcePolicyMode, WorkflowEvent,
};

type R = Result<(), Box<dyn std::error::Error>>;

fn connection() -> Result<RepositoryConnection, Box<dyn std::error::Error>> {
    Ok(RepositoryConnection::try_new(
        ConnectionId::parse("conn-org")?,
        42,
        "acme/repo".to_owned(),
        KeychainReference::parse("keychain:velnor.test/conn-org")?,
        ScaleSetSelector::try_new(
            "ubuntu-26.04-velnor-scale-set",
            &[
                "velnor".to_owned(),
                "ubuntu-26.04-velnor-scale-set".to_owned(),
            ],
        )?,
        SourcePolicy::try_new(
            SourcePolicyMode::BranchRestricted,
            vec!["main".to_owned()],
            Vec::new(),
            vec![WorkflowEvent::Push],
            Vec::new(),
        )?,
    )?)
}

fn group(id: i64, name: &str, is_default: bool) -> RunnerGroup {
    RunnerGroup {
        id,
        name: name.to_owned(),
        is_default,
    }
}

fn groups() -> Vec<RunnerGroup> {
    vec![
        group(1, "Default", true),
        group(4, "velnor-ci", false),
        group(9, "velnor-host", false),
    ]
}

struct Capture(Vec<SessionRequest>);

impl Transport for Capture {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.0.push(request.clone());
        Ok(Exchange {
            status: 201,
            body: br#"{"token":"AAAA","expires_at":"2030-01-01T00:00:00Z"}"#.to_vec(),
        })
    }
}

fn token_request(
    connection: &RepositoryConnection,
) -> Result<SessionRequest, Box<dyn std::error::Error>> {
    let mut transport = Capture(Vec::new());
    let target = connection.registration_target()?;
    let call = RegistrationTokenCall {
        scope: target.scope(),
        pat: "pat",
    };
    let _token = registration_token(&mut transport, &call)?;
    transport.0.pop().ok_or_else(|| "no request".into())
}

#[test]
fn repository_scope_is_the_default() -> R {
    let connection = connection()?;
    assert_eq!(
        connection.registration_target()?,
        RegistrationTarget::Repository {
            owner: "acme",
            repo: "repo"
        }
    );
    assert_eq!(
        connection.registration_target()?.config_url(),
        "https://github.com/acme/repo"
    );
    let request = token_request(&connection)?;
    assert_eq!(request.method, Method::Post);
    assert_eq!(
        request.path,
        "/repos/acme/repo/actions/runners/registration-token"
    );
    Ok(())
}

#[test]
fn organization_scope_uses_the_org_registration_endpoint() -> R {
    let connection = connection()?.with_organization_group("acme", Some("velnor-host"))?;
    assert_eq!(
        connection.registration_target()?,
        RegistrationTarget::Organization { org: "acme" }
    );
    assert_eq!(
        connection.registration_target()?.config_url(),
        "https://github.com/acme"
    );
    let request = token_request(&connection)?;
    assert_eq!(request.method, Method::Post);
    assert_eq!(
        request.path,
        "/orgs/acme/actions/runners/registration-token"
    );
    Ok(())
}

#[test]
fn repository_connection_selects_the_default_group() -> R {
    assert_eq!(connection()?.select_runner_group(&groups())?, 1);
    Ok(())
}

#[test]
fn named_group_resolves_to_its_id() -> R {
    let connection = connection()?.with_organization_group("acme", Some("velnor-host"))?;
    assert_eq!(connection.select_runner_group(&groups())?, 9);
    let no_name = connection_with_org_only()?;
    assert_eq!(no_name.select_runner_group(&groups())?, 1);
    Ok(())
}

fn connection_with_org_only() -> Result<RepositoryConnection, Box<dyn std::error::Error>> {
    Ok(connection()?.with_organization_group("acme", None)?)
}

#[test]
fn unknown_duplicate_and_invalid_groups_fail_closed() -> R {
    let connection = connection()?.with_organization_group("acme", Some("velnor-host"))?;
    assert!(connection.select_runner_group(&groups()[..2]).is_err());
    assert!(connection.select_runner_group(&[]).is_err());
    let mut duplicated = groups();
    duplicated.push(group(10, "velnor-host", false));
    assert!(connection.select_runner_group(&duplicated).is_err());
    let invalid = vec![group(0, "velnor-host", false)];
    assert!(connection.select_runner_group(&invalid).is_err());
    let lowercase = vec![group(9, "Velnor-Host", false)];
    assert!(connection.select_runner_group(&lowercase).is_err());
    Ok(())
}

#[test]
fn group_without_organization_and_foreign_organization_are_rejected() -> R {
    let mut named = connection()?;
    named.runner_group = Some("velnor-host".to_owned());
    assert!(named.validate().is_err());
    assert!(
        connection()?
            .with_organization_group("other", None)
            .is_err()
    );
    assert!(
        connection()?
            .with_organization_group("acme", Some(" "))
            .is_err()
    );
    Ok(())
}

#[test]
fn identity_bytes_are_unchanged_for_repository_connections() -> R {
    let plain = String::from_utf8(connection()?.canonical_identity_bytes()?)?;
    assert!(!plain.contains("organization") && !plain.contains("runner_group"));
    let org = connection()?.with_organization_group("acme", Some("velnor-host"))?;
    let text = String::from_utf8(org.canonical_identity_bytes()?)?;
    assert!(
        text.contains(r#""organization":"acme""#)
            && text.contains(r#""runner_group":"velnor-host""#)
    );
    Ok(())
}

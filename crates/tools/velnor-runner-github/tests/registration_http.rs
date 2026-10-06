//! Registration token and admin connection. No sockets.

#[path = "regwire/mod.rs"]
mod regwire;

use velnor_runner_github::{
    AdminConnectionCall, CAPACITY_HEADER, Method, RegistrationScope, RegistrationTokenCall,
    SessionError, WireError, admin_connection, enterprise_registration_token_path,
    organization_registration_token_path, registration_token, repository_registration_token_path,
};

use regwire::{Script, exchange, header, show};

const REG: &str = "reg-token-canary";
const PAT: &str = "pat-canary-value";
const ADMIN: &str = "admin-secret-canary";
const SERVICE: &str = "https://actions.example/tenant";

#[test]
fn empty_credentials_do_not_call_transport() {
    let mut script = Script::once(201, r#"{"token":"issued"}"#);
    let pat = registration_token(
        &mut script,
        &RegistrationTokenCall {
            scope: RegistrationScope::Repository {
                owner: "acme",
                repo: "widget",
            },
            pat: "",
        },
    );
    assert_eq!(
        pat.err(),
        Some(SessionError::Wire(WireError::RegistrationRejected))
    );
    let org = registration_token(
        &mut script,
        &RegistrationTokenCall {
            scope: RegistrationScope::Organization { org: "" },
            pat: PAT,
        },
    );
    assert_eq!(
        org.err(),
        Some(SessionError::Wire(WireError::RegistrationRejected))
    );
    let enterprise = registration_token(
        &mut script,
        &RegistrationTokenCall {
            scope: RegistrationScope::Enterprise { enterprise: "" },
            pat: PAT,
        },
    );
    assert_eq!(
        enterprise.err(),
        Some(SessionError::Wire(WireError::RegistrationRejected))
    );
    let repo = registration_token(
        &mut script,
        &RegistrationTokenCall {
            scope: RegistrationScope::Repository {
                owner: "",
                repo: "widget",
            },
            pat: PAT,
        },
    );
    assert_eq!(
        repo.err(),
        Some(SessionError::Wire(WireError::RegistrationRejected))
    );
    assert_eq!(script.seen.len(), 0);
    let mut admin_script = Script::once(200, r#"{"url":"https://x","token":"y"}"#);
    let admin = admin_connection(
        &mut admin_script,
        &AdminConnectionCall {
            config_url: "https://github.com/acme/widget",
            registration_token: "",
        },
    );
    assert_eq!(
        admin.err(),
        Some(SessionError::Wire(WireError::RegistrationRejected))
    );
    assert_eq!(admin_script.seen.len(), 0);
}

#[test]
fn registration_paths_reject_empty_components() {
    assert_eq!(
        repository_registration_token_path("acme", "widget").ok(),
        Some("/repos/acme/widget/actions/runners/registration-token".to_owned())
    );
    assert_eq!(
        organization_registration_token_path("acme").ok(),
        Some("/orgs/acme/actions/runners/registration-token".to_owned())
    );
    assert_eq!(
        enterprise_registration_token_path("contoso").ok(),
        Some("/enterprises/contoso/actions/runners/registration-token".to_owned())
    );
    assert_eq!(
        repository_registration_token_path("", "widget"),
        Err(WireError::RegistrationRejected)
    );
    assert_eq!(
        repository_registration_token_path("acme", ""),
        Err(WireError::RegistrationRejected)
    );
    assert_eq!(
        organization_registration_token_path(""),
        Err(WireError::RegistrationRejected)
    );
    assert_eq!(
        enterprise_registration_token_path(""),
        Err(WireError::RegistrationRejected)
    );
}

#[test]
fn registration_token_posts_201_and_hides_the_token() -> Result<(), String> {
    let secret = "issued-reg-token";
    let mut script = Script::once(
        201,
        &format!(r#"{{"token":"{secret}","expires_at":"2026"}}"#),
    );
    let token = show(registration_token(
        &mut script,
        &RegistrationTokenCall {
            scope: RegistrationScope::Repository {
                owner: "acme",
                repo: "widget",
            },
            pat: PAT,
        },
    ))?;
    let request = script.seen.first().ok_or("request")?;
    assert_eq!(request.method, Method::Post);
    assert_eq!(
        request.path,
        "/repos/acme/widget/actions/runners/registration-token"
    );
    assert_eq!(request.query, None);
    assert_eq!(request.body.len(), 0);
    assert_eq!(
        header(request, "Content-Type"),
        Some("application/vnd.github.v3+json")
    );
    let bearer = format!("Bearer {PAT}");
    assert_eq!(header(request, "Authorization"), Some(bearer.as_str()));
    assert_eq!(header(request, "User-Agent"), Some("velnor-host"));
    assert!(header(request, CAPACITY_HEADER).is_none());
    assert_eq!(token.expose(), secret);
    let rendered = format!("{token:?}");
    assert!(!rendered.contains(secret));
    assert!(!format!("{request:?}").contains(PAT));
    Ok(())
}

#[test]
fn missing_registration_token_is_rejected() {
    for body in [r#"{"token":null}"#, "{}", r#"{"token":""}"#] {
        let mut script = Script::once(201, body);
        let err = registration_token(
            &mut script,
            &RegistrationTokenCall {
                scope: RegistrationScope::Repository {
                    owner: "acme",
                    repo: "widget",
                },
                pat: PAT,
            },
        );
        assert_eq!(
            err.err(),
            Some(SessionError::Wire(WireError::RegistrationRejected))
        );
        assert_eq!(script.seen.len(), 1);
    }
    let mut denied = Script::once(401, "");
    let err = registration_token(
        &mut denied,
        &RegistrationTokenCall {
            scope: RegistrationScope::Organization { org: "acme" },
            pat: PAT,
        },
    );
    assert_eq!(
        err.err(),
        Some(SessionError::Wire(WireError::UnexpectedStatus))
    );
    assert_eq!(denied.seen.len(), 1);
}

#[test]
fn admin_connection_remote_auth_is_redacted() -> Result<(), String> {
    let mut script = Script::once(200, &format!(r#"{{"url":"{SERVICE}","token":"{ADMIN}"}}"#));
    let connection = show(admin_connection(
        &mut script,
        &AdminConnectionCall {
            config_url: "https://github.com/acme/widget",
            registration_token: REG,
        },
    ))?;
    let request = script.seen.first().ok_or("request")?;
    assert_eq!(request.method, Method::Post);
    assert_eq!(request.path, "/actions/runner-registration");
    assert_eq!(request.query, None);
    assert_eq!(
        request.body,
        br#"{"url":"https://github.com/acme/widget","runner_event":"register"}"#
    );
    let authorization = header(request, "Authorization").ok_or("auth")?;
    assert_eq!(authorization, format!("RemoteAuth {REG}"));
    assert!(authorization.starts_with("RemoteAuth "));
    assert_eq!(header(request, "Content-Type"), Some("application/json"));
    assert_eq!(header(request, "User-Agent"), Some("velnor-host"));
    let debug = format!("{request:?}");
    assert!(!debug.contains(REG));
    assert!(!format!("{connection:?}").contains(ADMIN));
    assert!(!format!("{connection:?}").contains(SERVICE));
    assert_eq!(connection.expose_url(), SERVICE);
    assert_eq!(connection.expose_token(), ADMIN);
    Ok(())
}

#[test]
fn admin_connection_retries_401_once() {
    let mut script = Script::replies(vec![
        Ok(exchange(401, "")),
        Ok(exchange(401, r#"{"url":"https://x","token":"y"}"#)),
    ]);
    let err = admin_connection(
        &mut script,
        &AdminConnectionCall {
            config_url: "https://github.com/acme/widget",
            registration_token: REG,
        },
    );
    assert_eq!(
        err.err(),
        Some(SessionError::Wire(WireError::UnexpectedStatus))
    );
    assert_eq!(script.seen.len(), 2);
    assert_eq!(
        header(&script.seen[0], "Authorization"),
        header(&script.seen[1], "Authorization")
    );
}

#[test]
fn admin_connection_retries_403_then_stops() {
    let mut script = Script::replies(vec![Ok(exchange(403, "")), Ok(exchange(403, ""))]);
    let err = admin_connection(
        &mut script,
        &AdminConnectionCall {
            config_url: "https://github.com/acme/widget",
            registration_token: REG,
        },
    );
    assert_eq!(err.err(), Some(SessionError::Wire(WireError::Forbidden)));
    assert_eq!(script.seen.len(), 2);
}

#[test]
fn admin_connection_empty_url_is_rejected() {
    let mut script = Script::once(200, &format!(r#"{{"url":"","token":"{ADMIN}"}}"#));
    let err = admin_connection(
        &mut script,
        &AdminConnectionCall {
            config_url: "https://github.com/acme/widget",
            registration_token: REG,
        },
    );
    let err = err.err();
    assert_eq!(
        err,
        Some(SessionError::Wire(WireError::RegistrationRejected))
    );
    assert_eq!(script.seen.len(), 1);
    let rendered = format!("{err:?}");
    assert!(!rendered.contains(ADMIN));
}

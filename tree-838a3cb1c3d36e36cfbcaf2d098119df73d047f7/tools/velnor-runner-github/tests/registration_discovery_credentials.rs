//! Malformed credentials returned by a dispatched discovery POST stay uncertain.

#[path = "common/discovery.rs"]
mod discovery_test_support;

use discovery_test_support::{
    ADMIN_TOKEN, HOST_CREDENTIAL, Intents, OWNER, REGISTRATION_TOKEN, REPOSITORY, Script, header,
    reply, repository_admin,
};
use std::rc::Rc;
use velnor_runner_github::{
    DiscoveryCredentialOutcome, DiscoveryIntentId, Method, SessionError,
    exchange_repository_discovery_admin_once, issue_repository_discovery_token,
    read_repository_admin_evidence,
};

const INVALID_TOKEN_VALUES: [&str; 2] = ["bad token", r"bad\u0001token"];

#[test]
fn malformed_registration_credentials_remain_uncertain_without_exchange() {
    for invalid_token in INVALID_TOKEN_VALUES {
        let mut script = Script::new(vec![
            reply(200, &repository_admin(true, Some(true))),
            reply(201, &format!(r#"{{"token":"{invalid_token}"}}"#)),
            reply(
                200,
                &format!(r#"{{"url":"https://actions.example/org","token":"{ADMIN_TOKEN}"}}"#),
            ),
        ]);
        let evidence =
            read_repository_admin_evidence(&mut script, OWNER, REPOSITORY, HOST_CREDENTIAL)
                .expect("private repository metadata is valid");
        let mut intents = Intents::new(Rc::clone(&script.events));

        let error =
            issue_repository_discovery_token(&mut script, evidence, HOST_CREDENTIAL, &mut intents)
                .expect_err("malformed issued token must fail closed");

        assert_eq!(error, SessionError::Uncertain);
        assert!(!format!("{error:?}").contains("bad token"));
        assert!(!format!("{error:?}").contains("bad\u{1}token"));
        assert_eq!(script.seen.len(), 2);
        assert_eq!(script.seen[1].method, Method::Post);
        assert_eq!(
            header(&script.seen[1], "Authorization"),
            Some("Bearer host-credential-canary")
        );
        assert_eq!(
            intents.outcomes,
            [(
                DiscoveryIntentId::new(1).expect("positive intent"),
                DiscoveryCredentialOutcome::Uncertain,
            )]
        );
    }
}

#[test]
fn malformed_admin_credentials_remain_uncertain_without_metadata_get() {
    for invalid_token in INVALID_TOKEN_VALUES {
        let mut script = Script::new(vec![
            reply(200, &repository_admin(true, Some(true))),
            reply(201, &format!(r#"{{"token":"{REGISTRATION_TOKEN}"}}"#)),
            reply(
                200,
                &format!(r#"{{"url":"https://actions.example/org","token":"{invalid_token}"}}"#),
            ),
            reply(200, r#"{"count":0,"value":[]}"#),
        ]);
        let evidence =
            read_repository_admin_evidence(&mut script, OWNER, REPOSITORY, HOST_CREDENTIAL)
                .expect("private repository metadata is valid");
        let mut intents = Intents::new(Rc::clone(&script.events));
        let registration =
            issue_repository_discovery_token(&mut script, evidence, HOST_CREDENTIAL, &mut intents)
                .expect("registration token is valid");

        let error =
            exchange_repository_discovery_admin_once(&mut script, registration, &mut intents)
                .expect_err("malformed issued admin token must fail closed");

        assert_eq!(error, SessionError::Uncertain);
        assert!(!format!("{error:?}").contains("bad token"));
        assert!(!format!("{error:?}").contains("bad\u{1}token"));
        assert_eq!(script.seen.len(), 3);
        assert_eq!(script.seen[2].method, Method::Post);
        assert_eq!(
            header(&script.seen[2], "Authorization"),
            Some("RemoteAuth registration-token-canary")
        );
        assert_eq!(
            intents.outcomes,
            [
                (
                    DiscoveryIntentId::new(1).expect("positive intent"),
                    DiscoveryCredentialOutcome::Succeeded,
                ),
                (
                    DiscoveryIntentId::new(2).expect("positive intent"),
                    DiscoveryCredentialOutcome::Uncertain,
                ),
            ]
        );
    }
}

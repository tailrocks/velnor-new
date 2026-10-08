use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use velnor_runner_github::{
    DiscoveryCredentialOutcome, DiscoveryCredentialStep, DiscoveryIntentId, DiscoveryIntentStore,
    DiscoveryTransport, Exchange, MessageQueueRoute, SessionError, SessionRequest, Transport,
    TransportFail, WireError,
};

pub(crate) const OWNER: &str = "ChainArgos";
pub(crate) const REPOSITORY: &str = "java-monorepo";
pub(crate) const HOST_CREDENTIAL: &str = "host-credential-canary";
pub(crate) const REGISTRATION_TOKEN: &str = "registration-token-canary";
pub(crate) const ADMIN_TOKEN: &str = "admin-token-canary";

pub(crate) struct Script {
    pub(crate) replies: VecDeque<Result<Exchange, TransportFail>>,
    pub(crate) seen: Vec<SessionRequest>,
    pub(crate) events: Rc<RefCell<Vec<String>>>,
    pub(crate) api_bindings: usize,
    pub(crate) actions_bindings: Vec<String>,
    pub(crate) reject_binding: bool,
    pub(crate) reject_actions_binding: bool,
}

impl Script {
    pub(crate) fn new(replies: Vec<Exchange>) -> Self {
        Self::with_events(replies, Rc::default())
    }

    pub(crate) fn with_events(replies: Vec<Exchange>, events: Rc<RefCell<Vec<String>>>) -> Self {
        Self::from_results(replies.into_iter().map(Ok).collect(), events)
    }

    pub(crate) fn from_results(
        replies: Vec<Result<Exchange, TransportFail>>,
        events: Rc<RefCell<Vec<String>>>,
    ) -> Self {
        Self {
            replies: replies.into(),
            seen: Vec::new(),
            events,
            api_bindings: 0,
            actions_bindings: Vec::new(),
            reject_binding: false,
            reject_actions_binding: false,
        }
    }
}

impl Transport for Script {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.events
            .borrow_mut()
            .push(format!("request:{:?}:{}", request.method, request.path));
        self.seen.push(request.clone());
        self.replies.pop_front().ok_or(TransportFail::Reset)?
    }
}

impl DiscoveryTransport for Script {
    fn bind_github_api_origin(&mut self) -> Result<(), SessionError> {
        self.events.borrow_mut().push("bind-api".to_owned());
        if self.reject_binding {
            return Err(WireError::RegistrationRejected.into());
        }
        self.api_bindings += 1;
        Ok(())
    }

    fn bind_actions_service_origin(&mut self, url: &str) -> Result<(), SessionError> {
        self.events.borrow_mut().push("bind-actions".to_owned());
        if self.reject_binding || self.reject_actions_binding {
            return Err(WireError::RegistrationRejected.into());
        }
        self.actions_bindings.push(url.to_owned());
        Ok(())
    }

    fn bind_message_queue_origin(&mut self, url: &str) -> Result<MessageQueueRoute, SessionError> {
        self.events.borrow_mut().push("bind-queue".to_owned());
        if self.reject_binding || self.reject_actions_binding {
            return Err(WireError::RegistrationRejected.into());
        }
        self.actions_bindings.push(url.to_owned());
        let rest = url
            .strip_prefix("https://")
            .ok_or(SessionError::Uncertain)?;
        let (authority, suffix) = rest.split_once('/').unwrap_or((rest, ""));
        if authority.is_empty() || authority.contains('@') || authority.contains('?') {
            return Err(SessionError::Uncertain);
        }
        let (path, query) = suffix.split_once('?').unwrap_or((suffix, ""));
        MessageQueueRoute::from_parts(
            if path.is_empty() {
                "/".to_owned()
            } else {
                format!("/{path}")
            },
            (!query.is_empty()).then(|| query.to_owned()),
        )
        .map_err(Into::into)
    }
}

pub(crate) struct Intents {
    pub(crate) events: Rc<RefCell<Vec<String>>>,
    next_id: u64,
    pub(crate) before: Vec<(DiscoveryCredentialStep, i64, String, DiscoveryIntentId)>,
    pub(crate) outcomes: Vec<(DiscoveryIntentId, DiscoveryCredentialOutcome)>,
    pub(crate) fail_before: bool,
    pub(crate) fail_outcome: bool,
}

impl Intents {
    pub(crate) fn new(events: Rc<RefCell<Vec<String>>>) -> Self {
        Self {
            events,
            next_id: 1,
            before: Vec::new(),
            outcomes: Vec::new(),
            fail_before: false,
            fail_outcome: false,
        }
    }
}

impl DiscoveryIntentStore for Intents {
    fn persist_before(
        &mut self,
        step: DiscoveryCredentialStep,
        repository_id: i64,
        full_name: &str,
    ) -> Result<DiscoveryIntentId, SessionError> {
        self.events.borrow_mut().push(format!("intent:{step:?}"));
        if self.fail_before {
            return Err(SessionError::Uncertain);
        }
        let Some(id) = DiscoveryIntentId::new(self.next_id) else {
            return Err(SessionError::Uncertain);
        };
        let Some(next_id) = self.next_id.checked_add(1) else {
            return Err(SessionError::Uncertain);
        };
        self.next_id = next_id;
        self.before
            .push((step, repository_id, full_name.to_owned(), id));
        Ok(id)
    }

    fn record_outcome(
        &mut self,
        id: DiscoveryIntentId,
        outcome: DiscoveryCredentialOutcome,
    ) -> Result<(), SessionError> {
        self.events
            .borrow_mut()
            .push(format!("outcome:{}:{outcome:?}", id.get()));
        if self.fail_outcome {
            return Err(SessionError::Uncertain);
        }
        self.outcomes.push((id, outcome));
        Ok(())
    }
}

pub(crate) fn reply(status: u16, body: &str) -> Exchange {
    Exchange {
        status,
        body: body.as_bytes().to_vec(),
    }
}

pub(crate) fn header<'a>(request: &'a SessionRequest, name: &str) -> Option<&'a str> {
    request
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

pub(crate) fn repository_admin(private: bool, admin: Option<bool>) -> String {
    let permission = admin.map_or_else(String::new, |value| {
        format!(r#", "permissions":{{"admin":{value}}}"#)
    });
    format!(
        r#"{{"id":829618808,"full_name":"{OWNER}/{REPOSITORY}","private":{private}{permission}}}"#
    )
}

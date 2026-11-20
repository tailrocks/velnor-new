// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use ureq::http::Uri;
use velnor_model::GitRepository;

pub const API_HOST: &str = "api.github.com";

pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

pub const DEFAULT_TOTAL_SECONDS: u64 = 30;

#[derive(Debug, Clone, Copy)]
pub struct Deadline(Instant);

impl Deadline {
    #[must_use]
    pub fn after(limit: Duration) -> Self {
        Self(Instant::now() + limit)
    }

    #[must_use]
    pub fn request_timeout(self) -> Option<Duration> {
        let left = self.0.saturating_duration_since(Instant::now());
        (!left.is_zero()).then(|| left.min(REQUEST_TIMEOUT))
    }
}

pub const VERDICTS_VARIABLE: &str = "VELNOR_PUBLIC_VERDICTS";

#[must_use]
pub fn bearer_allowed(url: &str) -> bool {
    let Ok(uri) = url.parse::<Uri>() else {
        return false;
    };
    let Some(authority) = uri.authority() else {
        return false;
    };
    uri.scheme_str() == Some("https")
        && (authority.as_str() == API_HOST || authority.as_str() == "api.github.com:443")
}

#[must_use]
pub fn key(repository: &GitRepository) -> String {
    format!("{}/{}", repository.owner, repository.name).to_ascii_lowercase()
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Verdicts(BTreeMap<String, bool>);

impl Verdicts {
    pub fn record(&mut self, repository: &GitRepository, public: bool) {
        self.0.insert(key(repository), public);
    }

    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        serde_json::from_str::<BTreeMap<String, bool>>(text)
            .ok()
            .map(|found| {
                Self(
                    found
                        .into_iter()
                        .map(|(name, public)| (name.to_ascii_lowercase(), public))
                        .collect(),
                )
            })
    }

    #[must_use]
    pub fn proves_public(&self, repository: &GitRepository) -> bool {
        self.0.get(&key(repository)).copied() == Some(true)
    }

    #[must_use]
    pub fn render(&self) -> String {
        serde_json::to_string(&self.0).unwrap_or_else(|_| "{}".to_owned())
    }
}

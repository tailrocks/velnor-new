// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The rules of the public-repository probe that `cache decide` and `cache public-sources` share.

use std::collections::BTreeMap;

use ureq::http::Uri;
use velnor_model::GitRepository;

/// The job's token goes to this host and no other.
pub const API_HOST: &str = "api.github.com";

/// Name of the environment variable that carries the verdicts of this run into a `decide` step.
pub const VERDICTS_VARIABLE: &str = "VELNOR_PUBLIC_VERDICTS";

/// Whether a request for `url` may carry the job's token: `https` to exactly `api.github.com`,
/// on the default port or 443, with no credentials in the URL. Nothing else gets the token, not a
/// look-alike host, a longer host, plain http or another port.
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

/// `owner/name` in lower case, the key of a verdict.
#[must_use]
pub fn key(repository: &GitRepository) -> String {
    format!("{}/{}", repository.owner, repository.name).to_ascii_lowercase()
}

/// Which repositories were found public earlier in this workflow run.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Verdicts(BTreeMap<String, bool>);

impl Verdicts {
    pub fn record(&mut self, repository: &GitRepository, public: bool) {
        self.0.insert(key(repository), public);
    }

    /// Reads the JSON object `{"owner/name": true}`; text that is not exactly that is `None`, and
    /// the caller probes for itself.
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

    /// The verdict is a skip only when it says public; anything else is probed again.
    #[must_use]
    pub fn proves_public(&self, repository: &GitRepository) -> bool {
        self.0.get(&key(repository)).copied() == Some(true)
    }

    #[must_use]
    pub fn render(&self) -> String {
        serde_json::to_string(&self.0).unwrap_or_else(|_| "{}".to_owned())
    }
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

struct OwnerRegistry<'a> {
    base: &'a Registry,
    login: &'a str,
    linked: Value,
}

impl CrateRegistryRead for OwnerRegistry<'_> {
    fn get(&self, path: &str) -> Result<CrateRegistryResponse, OrchestratorError> {
        let body = if path == "/api/v1/crates/demo/owners" {
            Some(json!({"users":[{"id":370_532,"login":self.login,"kind":"user"}]}))
        } else if path == format!("/api/v1/users/{}?include=linked_accounts", self.login) {
            Some(self.linked.clone())
        } else {
            None
        };
        match body {
            Some(body) => Ok(CrateRegistryResponse {
                status: 200,
                body: body.to_string().into_bytes(),
            }),
            None => self.base.get(path),
        }
    }
}

#[test]
fn reviewer_must_be_the_stable_github_account_linked_to_the_registry_owner()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let (intent, request, github, registry) = make_fixture(temp.path())?;
    let linked = |id: &str| json!({"user":{"id":370_532,"login":"owner"},"linked_accounts":[{"provider":"github","account_id":id,"login":"renamed-account"}]});
    let spoofed = OwnerRegistry {
        base: &registry,
        login: "owner",
        linked: linked("999"),
    };
    assert!(prepare_crate_release(&intent, &request, &github, &spoofed, temp.path()).is_err());
    let legitimate = OwnerRegistry {
        base: &registry,
        login: "owner",
        linked: linked("139017"),
    };
    assert!(prepare_crate_release(&intent, &request, &github, &legitimate, temp.path()).is_ok());
    let multiple = OwnerRegistry {
        base: &registry,
        login: "owner",
        linked: json!({"user":{"id":370_532,"login":"owner"},"linked_accounts":[{"provider":"github","account_id":"139017","login":"renamed-account"},{"provider":"github","account_id":"999","login":"other"}]}),
    };
    assert!(prepare_crate_release(&intent, &request, &github, &multiple, temp.path()).is_ok());
    for bad in [
        json!({"user":{"id":370_533,"login":"owner"},"linked_accounts":[{"provider":"github","account_id":"139017","login":"owner"}]}),
        json!({"user":{"id":370_532,"login":"owner"},"linked_accounts":null}),
        json!({"user":{"id":370_532,"login":"other"},"linked_accounts":[{"provider":"github","account_id":"139017","login":"owner"}]}),
        json!({"user":{"id":370_532,"login":"owner"}}),
        json!({"user":{"id":370_532,"login":"owner"},"linked_accounts":[{"provider":"github","account_id":"0139017","login":"owner"}]}),
        json!({"user":{"id":370_532,"login":"owner"},"linked_accounts":[{"provider":"github","account_id":139_017,"login":"owner"}]}),
        json!({"user":{"id":370_532,"login":"owner"},"linked_accounts":[{"provider":"other","account_id":"139017","login":"owner"}]}),
        json!({"user":{"id":370_532,"login":"owner"},"linked_accounts":[{"provider":"github","account_id":"139017","login":"owner"},{"provider":"github","account_id":"139017","login":"other"}]}),
    ] {
        let registry = OwnerRegistry {
            base: &registry,
            login: "owner",
            linked: bad,
        };
        assert!(prepare_crate_release(&intent, &request, &github, &registry, temp.path()).is_err());
    }
    Ok(())
}

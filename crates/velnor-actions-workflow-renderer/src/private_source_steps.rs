// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{
    PRIVATE_FETCH_OPERATION, PRIVATE_FETCH_TOKEN_ENV, PRIVATE_REVOKE_OPERATION,
    PRIVATE_TOKEN_OUTPUT_EXPR, Step, StepId, StepKind, StepRole,
};

use crate::{RenderError, steps::CREATE_GITHUB_APP_TOKEN_USES};

pub const PRIVATE_MINT_NAME: &str = "Mint read-only token for private sources";
pub const PRIVATE_FETCH_NAME: &str = "Fetch private sources";
pub const PRIVATE_REVOKE_NAME: &str = "Revoke private-source token";
pub const PRIVATE_REPOSITORIES_ENV: &str = "VELNOR_PRIVATE_REPOSITORIES";
pub const PRIVATE_OWNER_ENV: &str = "VELNOR_PRIVATE_OWNER";
pub const PRIVATE_SOURCES_ENV: &str = "VELNOR_PRIVATE_SOURCES";
pub const PRIVATE_ROOTS_ENV: &str = "VELNOR_PRIVATE_FETCH_ROOTS";
pub const PRIVATE_SOURCE_KINDS: [&str; 4] =
    ["cargo", "git-trees", "maven-bundles", "release-assets"];
pub const PRIVATE_REVOKE_CONDITION: &str = "always() && steps.deps-token.outcome == 'success'";

#[derive(Debug, Clone, Copy)]
pub struct PrivateSourcesSpec<'a> {
    pub owner: &'a str,
    pub app_id_variable: &'a str,
    pub app_private_key_secret: &'a str,
    pub repositories: &'a [String],
    pub kinds: &'a [&'a str],
    pub roots: &'a [String],
    pub toolchain_env: &'a BTreeMap<String, String>,
}

fn bad(detail: &str) -> RenderError {
    RenderError::BadCommand(format!("private_sources:{detail}"))
}

fn is_name_part(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

fn is_handle(value: &str) -> bool {
    value.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
        && value.len() <= 100
        && value
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

impl PrivateSourcesSpec<'_> {
    fn validate(&self) -> Result<(), RenderError> {
        if !is_name_part(self.owner) {
            return Err(bad("owner"));
        }
        if !is_handle(self.app_id_variable) || !is_handle(self.app_private_key_secret) {
            return Err(bad("handle"));
        }
        if self.repositories.is_empty()
            || !self.repositories.windows(2).all(|pair| pair[0] < pair[1])
            || !self.repositories.iter().all(|name| is_name_part(name))
        {
            return Err(bad("repositories"));
        }
        if self.kinds.is_empty()
            || !self.kinds.windows(2).all(|pair| pair[0] < pair[1])
            || !self
                .kinds
                .iter()
                .all(|kind| PRIVATE_SOURCE_KINDS.contains(kind))
        {
            return Err(bad("kinds"));
        }
        if !self.roots.windows(2).all(|pair| pair[0] < pair[1])
            || !self.roots.iter().all(|root| is_root(root))
        {
            return Err(bad("roots"));
        }
        if self.toolchain_env.keys().any(|key| {
            crate::toolchain_env::is_denied_credential_key(key)
                || crate::toolchain_env::is_denied_endpoint_key(key)
                || crate::toolchain_env::is_fetch_redirect_key(key)
                || key.starts_with("VELNOR_")
        }) || self
            .toolchain_env
            .values()
            .any(|value| value.contains("${{"))
        {
            return Err(bad("toolchain_env"));
        }
        Ok(())
    }
}

fn is_root(value: &str) -> bool {
    value == "."
        || (!value.is_empty()
            && value.split('/').all(|segment| {
                !segment.is_empty()
                    && segment != "."
                    && segment != ".."
                    && segment
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
            }))
}

pub fn private_source_steps(spec: &PrivateSourcesSpec<'_>) -> Result<[Step; 3], RenderError> {
    spec.validate()?;
    let mint = Step {
        parallel: false,
        name: PRIVATE_MINT_NAME.to_owned(),
        id: Some(StepId::DepsToken),
        role: Some(StepRole::PrivateTokenMint),
        condition: None,
        kind: StepKind::Action {
            uses: CREATE_GITHUB_APP_TOKEN_USES.to_owned(),
            with: BTreeMap::from([
                (
                    "client-id".to_owned(),
                    format!("${{{{ vars.{} }}}}", spec.app_id_variable),
                ),
                (
                    "private-key".to_owned(),
                    format!("${{{{ secrets.{} }}}}", spec.app_private_key_secret),
                ),
                ("owner".to_owned(), spec.owner.to_owned()),
                ("repositories".to_owned(), spec.repositories.join(",")),
                ("permission-contents".to_owned(), "read".to_owned()),
                ("skip-token-revoke".to_owned(), "false".to_owned()),
            ]),
            env: BTreeMap::new(),
        },
    };
    let fetch = Step {
        parallel: false,
        name: PRIVATE_FETCH_NAME.to_owned(),
        id: None,
        role: Some(StepRole::PrivateFetch),
        condition: None,
        kind: StepKind::Internal {
            operation: PRIVATE_FETCH_OPERATION.to_owned(),
            env: {
                let mut env = spec.toolchain_env.clone();
                env.extend([
                    (
                        PRIVATE_FETCH_TOKEN_ENV.to_owned(),
                        PRIVATE_TOKEN_OUTPUT_EXPR.to_owned(),
                    ),
                    (PRIVATE_OWNER_ENV.to_owned(), spec.owner.to_owned()),
                    (
                        PRIVATE_REPOSITORIES_ENV.to_owned(),
                        spec.repositories.join(","),
                    ),
                    (PRIVATE_SOURCES_ENV.to_owned(), spec.kinds.join(",")),
                    (PRIVATE_ROOTS_ENV.to_owned(), spec.roots.join(",")),
                ]);
                env
            },
        },
    };
    let revoke = Step {
        parallel: false,
        name: PRIVATE_REVOKE_NAME.to_owned(),
        id: None,
        role: Some(StepRole::PrivateRevoke),
        condition: Some(PRIVATE_REVOKE_CONDITION.to_owned()),
        kind: StepKind::Internal {
            operation: PRIVATE_REVOKE_OPERATION.to_owned(),
            env: BTreeMap::from([(
                PRIVATE_FETCH_TOKEN_ENV.to_owned(),
                PRIVATE_TOKEN_OUTPUT_EXPR.to_owned(),
            )]),
        },
    };
    Ok([mint, fetch, revoke])
}

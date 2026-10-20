//! Mint, fetch and revoke steps for private cross-repository sources (HD-7).
//!
//! The three steps form one block ahead of every repository-controlled step:
//! the token is minted for an exact repository allowlist, used only by the
//! Velnor-owned fetch step, and revoked `always()` before repository code
//! runs. Nothing here executes a shell; the fetch and revoke steps are
//! internal operations of the digest-verified staged binary.

use std::collections::BTreeMap;

use velnor_actions_contract::{
    PRIVATE_FETCH_OPERATION, PRIVATE_FETCH_TOKEN_ENV, PRIVATE_REVOKE_OPERATION,
    PRIVATE_TOKEN_OUTPUT_EXPR, Step, StepId, StepKind, StepRole,
};

use crate::{RenderError, steps::CREATE_GITHUB_APP_TOKEN_USES};

/// Display name of the mint step.
pub const PRIVATE_MINT_NAME: &str = "Mint read-only token for private sources";
/// Display name of the fetch step.
pub const PRIVATE_FETCH_NAME: &str = "Fetch private sources";
/// Display name of the revoke step.
pub const PRIVATE_REVOKE_NAME: &str = "Revoke private-source token";
/// Env key listing the minted repository allowlist for the fetch step.
pub const PRIVATE_REPOSITORIES_ENV: &str = "VELNOR_PRIVATE_REPOSITORIES";
/// Env key naming the owner of the allowlisted repositories.
pub const PRIVATE_OWNER_ENV: &str = "VELNOR_PRIVATE_OWNER";
/// Condition of the revoke step: runs after any outcome once a token exists.
pub const PRIVATE_REVOKE_CONDITION: &str = "always() && steps.deps-token.outcome == 'success'";

/// Everything the three steps are rendered from.
#[derive(Debug, Clone, Copy)]
pub struct PrivateSourcesSpec<'a> {
    /// Owner of every allowlisted repository.
    pub owner: &'a str,
    /// Organization variable naming the App client id.
    pub app_id_variable: &'a str,
    /// Organization secret naming the App private key.
    pub app_private_key_secret: &'a str,
    /// Sorted, de-duplicated repository names under `owner`; never empty.
    pub repositories: &'a [String],
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
        Ok(())
    }
}

/// The mint, fetch and revoke steps, in execution order.
///
/// # Errors
///
/// Returns [`RenderError::BadCommand`] for a malformed owner, handle, or an
/// empty, unsorted or malformed repository list.
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
            env: BTreeMap::from([
                (
                    PRIVATE_FETCH_TOKEN_ENV.to_owned(),
                    PRIVATE_TOKEN_OUTPUT_EXPR.to_owned(),
                ),
                (PRIVATE_OWNER_ENV.to_owned(), spec.owner.to_owned()),
                (
                    PRIVATE_REPOSITORIES_ENV.to_owned(),
                    spec.repositories.join(","),
                ),
            ]),
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

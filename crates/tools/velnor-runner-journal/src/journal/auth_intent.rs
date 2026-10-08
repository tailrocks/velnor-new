//! Durable one-shot credential-discovery effect intents.

use crate::error::HostError;

use super::{IntentState, Journal, one_row};

const KIND: &str = "discovery-credential";
const DESTINATION: &str = "https://api.github.com";

/// Credential POST that must be fenced before dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryCredentialStep {
    /// Request a repository registration token.
    RepositoryRegistrationToken,
    /// Request an organization registration token for bounded metadata reads.
    OrganizationRegistrationToken,
    /// Exchange that one-shot token for a repository admin connection.
    ActionsAdminExchange,
}

impl DiscoveryCredentialStep {
    const fn as_str(self) -> &'static str {
        match self {
            Self::RepositoryRegistrationToken => "repository-registration-token",
            Self::OrganizationRegistrationToken => "organization-registration-token",
            Self::ActionsAdminExchange => "actions-admin-exchange",
        }
    }
}

/// Supported registration scope used in a scope-aware discovery intent.
///
/// Enterprise registration is intentionally absent: the current host has no
/// configured enterprise credential route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryCredentialScope<'a> {
    /// Repository registration scope.
    Repository {
        /// Exact repository owner.
        owner: &'a str,
        /// Exact repository name.
        repository: &'a str,
    },
    /// Organization registration scope.
    Organization {
        /// Exact organization login.
        organization: &'a str,
    },
}

/// Outcome of one credential POST after its pre-effect intent was committed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryCredentialOutcome {
    /// A successful response was parsed and retained by the caller.
    Succeeded,
    /// The service returned a definite rejection.
    Rejected,
    /// The request or durable result write may have succeeded.
    Uncertain,
}

impl Journal {
    /// Persist an auth-only credential POST intent before sending it.
    ///
    /// The GitHub destination, immutable repository ID, and operation form
    /// the replay fence. The canonical repository name is retained in the
    /// subject as audited metadata, but a rename cannot bypass an unresolved
    /// intent for the same repository ID and operation. Different operation
    /// types or repository IDs have independent intents.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for invalid identity, an unresolved
    /// prior intent, read-only use, or a database failure.
    pub async fn begin_discovery_credential_intent(
        &self,
        step: DiscoveryCredentialStep,
        repository_id: i64,
        canonical_full_name: &str,
    ) -> Result<i64, HostError> {
        if self.read_only
            || repository_id <= 0
            || step == DiscoveryCredentialStep::OrganizationRegistrationToken
        {
            return Err(HostError::Journal);
        }
        let subject = subject(step, repository_id, canonical_full_name)?;
        let scope = repository_scope_prefix(step, repository_id)?;
        self.insert_pending_intent(scope, subject).await
    }

    /// Persist a scoped credential POST intent before sending it.
    ///
    /// The replay key binds the fixed GitHub destination, exact scope kind and
    /// name, immutable target repository ID, and operation. The target full
    /// name remains audit metadata and does not weaken the identity fence.
    /// Organization registration-token issuance requires organization scope;
    /// repository-token issuance requires repository scope. Admin exchange is
    /// supported for either scope.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for an unsupported scope/step pairing,
    /// invalid identity, unresolved prior intent, read-only use, or a database
    /// failure.
    pub async fn begin_scoped_discovery_credential_intent(
        &self,
        step: DiscoveryCredentialStep,
        registration_scope: DiscoveryCredentialScope<'_>,
        target_repository_id: i64,
        target_repository_full_name: &str,
    ) -> Result<i64, HostError> {
        if self.read_only || target_repository_id <= 0 {
            return Err(HostError::Journal);
        }
        let (prefix, audit_scope_kind, audit_scope_name) = match registration_scope {
            DiscoveryCredentialScope::Repository { owner, repository }
                if step != DiscoveryCredentialStep::OrganizationRegistrationToken =>
            {
                validate_scope_component(owner)?;
                validate_scope_component(repository)?;
                // Repository scope is already identified by the immutable
                // repository ID plus destination and step in the legacy key.
                // Reuse that exact prefix so intents written through either
                // API fence each other, and so a rename/transfer cannot create
                // a fresh key for the same repository.
                (
                    repository_scope_prefix(step, target_repository_id)?,
                    "repository",
                    format!("{owner}/{repository}"),
                )
            }
            DiscoveryCredentialScope::Organization { organization }
                if step != DiscoveryCredentialStep::RepositoryRegistrationToken =>
            {
                validate_scope_component(organization)?;
                (
                    organization_scope_prefix(step, organization, target_repository_id)?,
                    "organization",
                    organization.to_owned(),
                )
            }
            _ => return Err(HostError::Journal),
        };
        validate_full_name(target_repository_full_name)?;
        let mut subject = prefix.clone();
        append_component(&mut subject, audit_scope_kind);
        append_component(&mut subject, &audit_scope_name.to_ascii_lowercase());
        append_component(
            &mut subject,
            &target_repository_full_name.to_ascii_lowercase(),
        );
        self.insert_pending_intent(prefix, subject).await
    }

    async fn insert_pending_intent(
        &self,
        scope: String,
        subject: String,
    ) -> Result<i64, HostError> {
        if self.read_only {
            return Err(HostError::Journal);
        }
        let conn = self.connection().await?;
        conn.execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = async {
            let mut rows = conn
                .query(
                    "SELECT 1 FROM intents WHERE kind = ?1 AND substr(subject, 1, length(?2)) = ?2 AND state IN ('pending', 'uncertain') ORDER BY id LIMIT 1",
                    (KIND, scope.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            let unresolved = rows.next().await.map_err(|_| HostError::Journal)?;
            drop(rows);
            if unresolved.is_some() {
                // The full subject stores the canonical name for audit, but
                // the prefix fence intentionally ignores that mutable name.
                // Exact replays and rename/transfer conflicts both fail closed.
                return Err(HostError::Journal);
            }
            conn.execute(
                "INSERT INTO intents (kind, subject, state, effect_state) VALUES (?1, ?2, 'pending', 'may_have_effect')",
                (KIND, subject.as_str()),
            )
            .await
            .map_err(|_| HostError::Journal)?;
            Ok(conn.last_insert_rowid())
        }
        .await;
        let end = if result.is_ok() {
            conn.execute("COMMIT", ()).await
        } else {
            conn.execute("ROLLBACK", ()).await
        };
        end.map_err(|_| HostError::Journal)?;
        result
    }

    /// Record the result of a credential POST without clearing uncertainty.
    ///
    /// Replaying the exact outcome is idempotent. A pending intent may advance
    /// once; an uncertain, completed, or rejected intent cannot be rewritten.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for a missing/non-discovery row,
    /// conflicting replay, read-only use, or a database failure.
    pub async fn record_discovery_credential_outcome(
        &self,
        id: i64,
        outcome: DiscoveryCredentialOutcome,
    ) -> Result<(), HostError> {
        if self.read_only || id <= 0 {
            return Err(HostError::Journal);
        }
        let target = match outcome {
            DiscoveryCredentialOutcome::Succeeded => IntentState::Done,
            DiscoveryCredentialOutcome::Rejected => IntentState::Failed,
            DiscoveryCredentialOutcome::Uncertain => IntentState::Uncertain,
        };
        let target_text = target.as_str();
        let conn = self.connection().await?;
        conn.execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = async {
            let mut rows = conn
                .query("SELECT kind, state FROM intents WHERE id = ?1", [id])
                .await
                .map_err(|_| HostError::Journal)?;
            let row = rows.next().await.map_err(|_| HostError::Journal)?;
            let Some(row) = row else {
                return Err(HostError::Journal);
            };
            let kind = row.get::<String>(0).map_err(|_| HostError::Journal)?;
            let state = row.get::<String>(1).map_err(|_| HostError::Journal)?;
            drop(rows);
            if kind != KIND {
                return Err(HostError::Journal);
            }
            if state == target_text {
                return Ok(());
            }
            if state != "pending" {
                return Err(HostError::Journal);
            }
            let changed = conn
                .execute(
                    "UPDATE intents SET state = ?1 WHERE id = ?2 AND kind = ?3 AND state = 'pending'",
                    (target_text, id, KIND),
                )
                .await
                .map_err(|_| HostError::Journal)?;
            one_row(changed)
        }
        .await;
        let end = if result.is_ok() {
            conn.execute("COMMIT", ()).await
        } else {
            conn.execute("ROLLBACK", ()).await
        };
        end.map_err(|_| HostError::Journal)?;
        result
    }
}

fn subject(
    step: DiscoveryCredentialStep,
    repository_id: i64,
    canonical_full_name: &str,
) -> Result<String, HostError> {
    validate_full_name(canonical_full_name)?;
    let name = canonical_full_name.to_ascii_lowercase();
    let mut output = repository_scope_prefix(step, repository_id)?;
    append_component(&mut output, &name);
    Ok(output)
}

fn validate_full_name(value: &str) -> Result<(), HostError> {
    if value.is_empty()
        || value.len() > 201
        || !value.is_ascii()
        || value.chars().any(char::is_whitespace)
        || value.chars().any(char::is_control)
        || value.split('/').count() != 2
        || value.split('/').any(str::is_empty)
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

fn validate_scope_component(value: &str) -> Result<(), HostError> {
    if value.is_empty()
        || value.len() > 100
        || !value.is_ascii()
        || value.contains('/')
        || value.chars().any(char::is_whitespace)
        || value.chars().any(char::is_control)
    {
        return Err(HostError::Journal);
    }
    Ok(())
}

fn repository_scope_prefix(
    step: DiscoveryCredentialStep,
    repository_id: i64,
) -> Result<String, HostError> {
    if repository_id <= 0 {
        return Err(HostError::Journal);
    }
    let parts = [
        DESTINATION.to_owned(),
        step.as_str().to_owned(),
        repository_id.to_string(),
    ];
    let mut output = String::from("discovery-v1:");
    for part in parts {
        append_component(&mut output, &part);
    }
    Ok(output)
}

fn organization_scope_prefix(
    step: DiscoveryCredentialStep,
    organization: &str,
    target_repository_id: i64,
) -> Result<String, HostError> {
    if target_repository_id <= 0 {
        return Err(HostError::Journal);
    }
    let parts = [
        DESTINATION.to_owned(),
        step.as_str().to_owned(),
        "organization".to_owned(),
        organization.to_ascii_lowercase(),
        target_repository_id.to_string(),
    ];
    let mut output = String::from("discovery-scope-v1:");
    for part in parts {
        append_component(&mut output, &part);
    }
    Ok(output)
}

fn append_component(output: &mut String, value: &str) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    output.push_str(&value.len().to_string());
    output.push('=');
    for byte in value.bytes() {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output.push(';');
}

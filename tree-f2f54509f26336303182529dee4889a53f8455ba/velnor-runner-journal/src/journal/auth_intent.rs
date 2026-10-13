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
    /// Exchange that one-shot token for a repository admin connection.
    ActionsAdminExchange,
}

impl DiscoveryCredentialStep {
    const fn as_str(self) -> &'static str {
        match self {
            Self::RepositoryRegistrationToken => "repository-registration-token",
            Self::ActionsAdminExchange => "actions-admin-exchange",
        }
    }
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
        if self.read_only || repository_id <= 0 {
            return Err(HostError::Journal);
        }
        let subject = subject(step, repository_id, canonical_full_name)?;
        let scope = scope_prefix(step, repository_id)?;
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
    if canonical_full_name.is_empty()
        || canonical_full_name.len() > 201
        || !canonical_full_name.is_ascii()
        || canonical_full_name.chars().any(char::is_whitespace)
        || canonical_full_name.chars().any(char::is_control)
        || canonical_full_name.split('/').count() != 2
        || canonical_full_name.split('/').any(str::is_empty)
    {
        return Err(HostError::Journal);
    }
    let name = canonical_full_name.to_ascii_lowercase();
    let mut output = scope_prefix(step, repository_id)?;
    append_component(&mut output, &name);
    Ok(output)
}

fn scope_prefix(step: DiscoveryCredentialStep, repository_id: i64) -> Result<String, HostError> {
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

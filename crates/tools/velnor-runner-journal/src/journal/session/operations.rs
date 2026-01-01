//! Atomic persistence transitions for controller Scale Set sessions.

use crate::error::HostError;

use crate::journal::{Journal, one_row};

use super::{KIND, ScaleSetSessionClosePermit, ScaleSetSessionIdentity, validate_session_id};
use store::{claim_close, finish_transaction, reserve_session, session_row};

mod store;

/// Result of the atomic one-session admission reservation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleSetSessionClaim {
    /// A fresh intent committed; exactly one session create may now be sent.
    Reserved(i64),
    /// A live or ambiguous prior reservation exists; it does not authorize replay.
    Existing(i64),
    /// Durable drain was committed before this new session could reserve.
    Draining,
}

/// Result of an atomic claim to close a known controller session.
#[derive(Debug, PartialEq, Eq)]
pub enum ScaleSetSessionCloseClaim {
    /// A durable one-shot close attempt was claimed before DELETE dispatch.
    Claimed(Box<ScaleSetSessionClosePermit>),
    /// The exact recorded session is already closed.
    Closed,
    /// No successful session create was recorded for this identity.
    NoSession,
    /// Another route or an uncertain prior operation still holds the singleton.
    Held,
    /// No journaled session exists for this exact identity.
    Missing,
}

impl Journal {
    /// Reserve the controller's single Scale Set session before its create POST.
    ///
    /// This transaction checks the durable drain fence and singleton active
    /// session intent. It does not inspect or consume worker launch capacity.
    /// `Existing` never authorizes another POST; it may represent a crash or
    /// an ambiguous create/close result.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for read-only use, corrupt singleton
    /// state, or database failure. An ambiguous commit must be treated as held.
    pub async fn reserve_scale_set_session_if_accepting(
        &self,
        identity: &ScaleSetSessionIdentity,
    ) -> Result<ScaleSetSessionClaim, HostError> {
        if self.read_only {
            return Err(HostError::Journal);
        }
        let conn = self.connection().await?;
        conn.execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = reserve_session(&conn, identity).await;
        let end = if result.is_ok() {
            conn.execute("COMMIT", ()).await
        } else {
            conn.execute("ROLLBACK", ()).await
        };
        end.map_err(|_| HostError::Journal)?;
        result
    }

    /// Persist the exact session ID returned by the one authorized create POST.
    ///
    /// Replaying the same ID is idempotent. A different ID or an unexpected
    /// state is rejected, and no worker-slot row is changed.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for a malformed ID, conflicting replay,
    /// read-only use, or database failure.
    pub async fn record_scale_set_session_created(
        &self,
        intent_id: i64,
        session_id: &str,
    ) -> Result<(), HostError> {
        validate_session_id(session_id)?;
        let conn = self.transaction_connection().await?;
        let result = async {
            let (state, stored, intent_state, effect_state, _) =
                session_row(&conn, intent_id).await?;
            if state == "open"
                && stored.as_deref() == Some(session_id)
                && intent_state == "pending"
                && effect_state == "may_have_effect"
            {
                return Ok(());
            }
            if state != "creating"
                || stored.is_some()
                || intent_state != "pending"
                || effect_state != "may_have_effect"
            {
                return Err(HostError::Journal);
            }
            one_row(
                conn.execute(
                    "UPDATE scale_set_sessions SET session_id = ?1, state = 'open' WHERE intent_id = ?2 AND state = 'creating' AND session_id IS NULL",
                    (session_id, intent_id),
                )
                .await
                .map_err(|_| HostError::Journal)?,
            )
        }
        .await;
        finish_transaction(&conn, result).await
    }

    /// Record a definitive create rejection, which proves no session exists.
    ///
    /// This releases only the session reservation. It never changes launch
    /// occupancy. Ambiguous errors must remain in `creating` instead.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] unless this is an exact creating intent
    /// with no recorded session ID, or if storage fails.
    pub async fn record_scale_set_session_rejected(&self, intent_id: i64) -> Result<(), HostError> {
        let conn = self.transaction_connection().await?;
        let result = async {
            let (state, stored, intent_state, effect_state, _) =
                session_row(&conn, intent_id).await?;
            if state == "closed"
                && stored.is_none()
                && intent_state == "failed"
                && effect_state == "definite_no_effect"
            {
                return Ok(());
            }
            if state != "creating"
                || stored.is_some()
                || intent_state != "pending"
                || effect_state != "may_have_effect"
            {
                return Err(HostError::Journal);
            }
            one_row(
                conn.execute(
                    "UPDATE intents SET state = 'failed', effect_state = 'definite_no_effect' WHERE id = ?1 AND kind = ?2 AND state = 'pending'",
                    (intent_id, KIND),
                )
                .await
                .map_err(|_| HostError::Journal)?,
            )?;
            one_row(
                conn.execute(
                    "UPDATE scale_set_sessions SET state = 'closed' WHERE intent_id = ?1 AND state = 'creating' AND session_id IS NULL",
                    [intent_id],
                )
                .await
                .map_err(|_| HostError::Journal)?,
            )
        }
        .await;
        finish_transaction(&conn, result).await
    }

    /// Atomically fence one DELETE for the exact known repository-scoped session.
    ///
    /// The close-attempt bit is committed before the caller sends DELETE. A
    /// dropped permit or uncertain response therefore remains held on restart.
    /// This current cleanup route deliberately refuses organization scope.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for read-only use, corrupt state, or a
    /// database failure. A mismatch is a successful `Held` outcome.
    pub async fn claim_scale_set_session_close(
        &self,
        identity: &ScaleSetSessionIdentity,
    ) -> Result<ScaleSetSessionCloseClaim, HostError> {
        if self.read_only {
            return Err(HostError::Journal);
        }
        if identity.registration_scope != "repository" {
            return Ok(ScaleSetSessionCloseClaim::Held);
        }
        let conn = self.transaction_connection().await?;
        let result = claim_close(&conn, identity).await;
        finish_transaction(&conn, result).await
    }

    /// Record an exact HTTP 204 close receipt for its previously claimed permit.
    ///
    /// An uncertain or non-204 response must not call this method. The same
    /// positive receipt is idempotent; no other route or session can use it.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] unless this exact permit is close-claimed
    /// and still bound to the stored session ID.
    pub async fn record_scale_set_session_closed(
        &self,
        permit: &ScaleSetSessionClosePermit,
    ) -> Result<(), HostError> {
        if !permit.delete_dispatch_started {
            return Err(HostError::Journal);
        }
        validate_session_id(&permit.session_id)?;
        let conn = self.transaction_connection().await?;
        let result = async {
            let (state, stored, intent_state, effect_state, close_attempted) =
                session_row(&conn, permit.intent_id).await?;
            if state == "closed"
                && stored.as_deref() == Some(permit.session_id.as_str())
                && close_attempted
                && intent_state == "done"
                && effect_state == "may_have_effect"
            {
                return Ok(());
            }
            if state != "open"
                || stored.as_deref() != Some(permit.session_id.as_str())
                || !close_attempted
                || intent_state != "pending"
                || effect_state != "may_have_effect"
            {
                return Err(HostError::Journal);
            }
            one_row(
                conn.execute(
                    "UPDATE scale_set_sessions SET state = 'closed' WHERE intent_id = ?1 AND state = 'open' AND close_attempted = 1 AND session_id = ?2",
                    (permit.intent_id, permit.session_id.as_str()),
                )
                .await
                .map_err(|_| HostError::Journal)?,
            )?;
            one_row(
                conn.execute(
                    "UPDATE intents SET state = 'done' WHERE id = ?1 AND kind = ?2 AND state = 'pending' AND effect_state = 'may_have_effect'",
                    (permit.intent_id, KIND),
                )
                .await
                .map_err(|_| HostError::Journal)?,
            )
        }
        .await;
        finish_transaction(&conn, result).await
    }

    async fn transaction_connection(&self) -> Result<turso::Connection, HostError> {
        if self.read_only {
            return Err(HostError::Journal);
        }
        let conn = self.connection().await?;
        conn.execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        Ok(conn)
    }
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::config::{ConnectionId, RepositoryConnection};
use crate::error::HostError;
use crate::journal::Journal;

use super::ConnectionJournal;
use super::storage::{
    connection_record, digest, has_ambiguous_legacy_work, has_connection_work, in_transaction,
    read_transaction,
};

impl Journal {
    pub(crate) async fn connection_scope(
        &self,
        binding: &RepositoryConnection,
    ) -> Result<ConnectionJournal, HostError> {
        let identity_bytes = binding
            .canonical_identity_bytes()
            .map_err(|_| HostError::Journal)?;
        if identity_bytes.is_empty() || identity_bytes.len() > 1_048_576 {
            return Err(HostError::Journal);
        }
        let identity = String::from_utf8(identity_bytes).map_err(|_| HostError::Journal)?;
        let connection_id = binding.connection_id.clone();
        let binding_digest = digest(identity.as_bytes());
        let id_text = connection_id.as_str().to_owned();
        let identity_text = identity.clone();
        in_transaction(self, async |connection| {
            if has_ambiguous_legacy_work(connection).await? {
                return Err(HostError::Journal);
            }
            let existing = connection_record(connection, &id_text).await?;
            match existing {
                Some((existing, false)) if existing == identity_text => Ok(()),
                Some(_) => Err(HostError::Journal),
                None => {
                    connection
                        .execute(
                            "INSERT INTO connection_scopes (connection_id, identity) VALUES (?1, ?2)",
                            (id_text, identity_text),
                        )
                        .await
                        .map_err(|_| HostError::Journal)?;
                    Ok(())
                }
            }
        })
        .await?;
        Ok(ConnectionJournal {
            journal: self.clone(),
            connection_id,
            identity,
            binding_digest,
        })
    }

    pub(crate) async fn existing_connection_scope(
        &self,
        binding: &RepositoryConnection,
    ) -> Result<ConnectionJournal, HostError> {
        let identity_bytes = binding
            .canonical_identity_bytes()
            .map_err(|_| HostError::Journal)?;
        if identity_bytes.is_empty() || identity_bytes.len() > 1_048_576 {
            return Err(HostError::Journal);
        }
        let identity = String::from_utf8(identity_bytes).map_err(|_| HostError::Journal)?;
        let connection_id = binding.connection_id.clone();
        let binding_digest = digest(identity.as_bytes());
        let id_text = connection_id.as_str().to_owned();
        let identity_text = identity.clone();
        read_transaction(self, async |connection| {
            if has_ambiguous_legacy_work(connection).await? {
                return Err(HostError::Journal);
            }
            match connection_record(connection, &id_text).await? {
                Some((existing, false)) if existing == identity_text => Ok(()),
                _ => Err(HostError::Journal),
            }
        })
        .await?;
        Ok(ConnectionJournal {
            journal: self.clone(),
            connection_id,
            identity,
            binding_digest,
        })
    }

    pub(crate) async fn reactivate_if_drained(
        &self,
        binding: &RepositoryConnection,
    ) -> Result<bool, HostError> {
        let identity_bytes = binding
            .canonical_identity_bytes()
            .map_err(|_| HostError::Journal)?;
        if identity_bytes.is_empty() || identity_bytes.len() > 1_048_576 {
            return Err(HostError::Journal);
        }
        let identity = String::from_utf8(identity_bytes).map_err(|_| HostError::Journal)?;
        let connection_id = binding.connection_id.as_str();
        in_transaction(self, async |connection| {
            if has_ambiguous_legacy_work(connection).await? {
                return Ok(false);
            }
            let Some((stored_identity, retired)) = connection_record(connection, connection_id).await?
            else {
                return Err(HostError::Journal);
            };
            if stored_identity != identity {
                return Err(HostError::Journal);
            }
            if has_connection_work(connection, connection_id).await? {
                return Ok(false);
            }
            if retired {
                connection
                    .execute(
                        "UPDATE connection_scopes SET retired = 0 WHERE connection_id = ?1 AND identity = ?2 AND retired = 1",
                        (connection_id, identity.as_str()),
                    )
                    .await
                    .map_err(|_| HostError::Journal)?;
            }
            Ok(true)
        })
        .await
    }

    pub(crate) async fn is_retired_and_drained(
        &self,
        binding: &RepositoryConnection,
    ) -> Result<bool, HostError> {
        let identity_bytes = binding
            .canonical_identity_bytes()
            .map_err(|_| HostError::Journal)?;
        if identity_bytes.is_empty() || identity_bytes.len() > 1_048_576 {
            return Err(HostError::Journal);
        }
        let identity = String::from_utf8(identity_bytes).map_err(|_| HostError::Journal)?;
        let connection_id = binding.connection_id.as_str();
        read_transaction(self, async |connection| {
            if has_ambiguous_legacy_work(connection).await? {
                return Ok(false);
            }
            let Some((stored_identity, retired)) =
                connection_record(connection, connection_id).await?
            else {
                return Ok(false);
            };
            if stored_identity != identity {
                return Err(HostError::Journal);
            }
            if !retired {
                return Ok(false);
            }
            Ok(!has_connection_work(connection, connection_id).await?)
        })
        .await
    }

    pub(crate) async fn retire_connection_if_drained(
        &self,
        binding: &RepositoryConnection,
    ) -> Result<bool, HostError> {
        let identity_bytes = binding
            .canonical_identity_bytes()
            .map_err(|_| HostError::Journal)?;
        if identity_bytes.is_empty() || identity_bytes.len() > 1_048_576 {
            return Err(HostError::Journal);
        }
        let identity = String::from_utf8(identity_bytes).map_err(|_| HostError::Journal)?;
        let connection_id = binding.connection_id.as_str();
        in_transaction(self, async |connection| {
            if has_ambiguous_legacy_work(connection).await? {
                return Ok(false);
            }
            let Some((stored_identity, retired)) = connection_record(connection, connection_id).await?
            else {
                return Ok(false);
            };
            if stored_identity != identity {
                return Err(HostError::Journal);
            }
            if has_connection_work(connection, connection_id).await? {
                return Ok(false);
            }
            if !retired {
                connection
                    .execute(
                        "UPDATE connection_scopes SET retired = 1 WHERE connection_id = ?1 AND identity = ?2 AND retired = 0",
                        (connection_id, identity.as_str()),
                    )
                    .await
                    .map_err(|_| HostError::Journal)?;
            }
            Ok(true)
        })
        .await
    }
}

impl ConnectionJournal {
    #[must_use]
    pub(crate) fn connection_id(&self) -> &ConnectionId {
        &self.connection_id
    }

    pub(crate) async fn is_drained(&self) -> Result<bool, HostError> {
        read_transaction(&self.journal, async |connection| {
            if !matches!(
                connection_record(connection, self.connection_id.as_str()).await?,
                Some((identity, _)) if identity == self.identity
            ) {
                return Err(HostError::Journal);
            }
            if has_ambiguous_legacy_work(connection).await? {
                return Ok(false);
            }
            let active = has_connection_work(connection, self.connection_id.as_str()).await?;
            Ok(!active)
        })
        .await
    }

    pub(in crate::journal) async fn validate_scope(
        &self,
        connection: &turso::Connection,
    ) -> Result<(), HostError> {
        if !self.validate_scope_record(connection).await? {
            return Err(HostError::Journal);
        }
        Ok(())
    }

    pub(in crate::journal) const fn journal_ref(&self) -> &Journal {
        &self.journal
    }

    pub(in crate::journal) fn binding_digest(&self) -> &str {
        &self.binding_digest
    }

    async fn validate_scope_record(
        &self,
        connection: &turso::Connection,
    ) -> Result<bool, HostError> {
        Ok(matches!(
            connection_record(connection, self.connection_id.as_str()).await?,
            Some((identity, false)) if identity == self.identity
        ))
    }
}

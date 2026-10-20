use serde::Serialize;

use crate::error::HostError;

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ConfirmedAcquire<'a> {
    Returned { request_ids: &'a [i64] },
}

pub(super) async fn validate_claim_responses(conn: &turso::Connection) -> Result<(), HostError> {
    let mut rows = conn
        .query(
            "SELECT c.witness_request_id, c.acquire_response_digest, auth.acquire_response_json, auth.acquire_response_digest, a.acquire_response_digest FROM physical_attempt_post_jit_retirement_claims c JOIN connection_request_authorizations auth ON auth.connection_id=c.connection_id AND auth.scale_set_id=c.scale_set_id AND auth.request_id=c.witness_request_id JOIN physical_attempts a ON a.lineage_id=c.lineage_id AND a.generation=c.generation AND a.reservation_id=c.reservation_id AND a.intent_id=c.intent_id WHERE auth.binding_digest=c.binding_digest AND auth.source_digest=c.source_digest AND auth.acquire_state='acquired'",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? {
        let witness_request_id = row.get::<i64>(0).map_err(|_| HostError::Journal)?;
        let claim_digest = row.get::<String>(1).map_err(|_| HostError::Journal)?;
        let response_json = row
            .get::<Option<String>>(2)
            .map_err(|_| HostError::Journal)?;
        let response_digest = row
            .get::<Option<String>>(3)
            .map_err(|_| HostError::Journal)?;
        let attempt_digest = row
            .get::<Option<String>>(4)
            .map_err(|_| HostError::Journal)?;
        if !confirmed_response_matches(
            witness_request_id,
            &claim_digest,
            response_json.as_deref(),
            response_digest.as_deref(),
            attempt_digest.as_deref(),
        ) {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}

fn confirmed_response_matches(
    witness_request_id: i64,
    claim_digest: &str,
    response_json: Option<&str>,
    response_digest: Option<&str>,
    attempt_digest: Option<&str>,
) -> bool {
    let Some(response_json) = response_json else {
        return false;
    };
    let Some(response_digest) = response_digest else {
        return false;
    };
    let Ok(expected_json) = serde_json::to_string(&ConfirmedAcquire::Returned {
        request_ids: &[witness_request_id],
    }) else {
        return false;
    };
    response_json == expected_json
        && digest(response_json.as_bytes()) == response_digest
        && response_digest == claim_digest
        && attempt_digest.is_none_or(|stored| stored == response_digest)
}

fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};

    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        use std::fmt::Write as _;
        let _written = write!(output, "{byte:02x}");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::confirmed_response_matches;

    #[test]
    fn exact_witness_response_accepts_null_creation_snapshot() {
        let json = r#"{"kind":"returned","request_ids":[73]}"#;
        let digest = super::digest(json.as_bytes());
        assert!(confirmed_response_matches(
            73,
            &digest,
            Some(json),
            Some(&digest),
            None
        ));
    }

    #[test]
    fn response_for_another_request_is_rejected() {
        let json = r#"{"kind":"returned","request_ids":[74]}"#;
        let digest = super::digest(json.as_bytes());
        assert!(!confirmed_response_matches(
            73,
            &digest,
            Some(json),
            Some(&digest),
            None
        ));
    }

    #[test]
    fn non_null_creation_snapshot_must_match_confirmed_response() {
        let json = r#"{"kind":"returned","request_ids":[73]}"#;
        let digest = super::digest(json.as_bytes());
        assert!(!confirmed_response_matches(
            73,
            &digest,
            Some(json),
            Some(&digest),
            Some(&"0".repeat(64)),
        ));
    }

    #[test]
    fn matching_creation_snapshot_is_retained_as_a_second_check() {
        let json = r#"{"kind":"returned","request_ids":[73]}"#;
        let digest = super::digest(json.as_bytes());
        assert!(confirmed_response_matches(
            73,
            &digest,
            Some(json),
            Some(&digest),
            Some(&digest),
        ));
    }
}

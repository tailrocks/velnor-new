use std::time::{Duration, UNIX_EPOCH};

use super::evidence_expiry;
use crate::policy::{
    PolicyGap, PoolAdmissionEvidence, PoolEvidenceSource, PoolEvidenceSourceStamp,
};

#[test]
fn proof_expiry_uses_oldest_source_and_route_observation() {
    let now = UNIX_EPOCH + Duration::from_secs(10_000);
    let oldest = now - Duration::from_secs(29);
    let sources = [
        PoolEvidenceSourceStamp {
            source: PoolEvidenceSource::RepositoryMetadataRest,
            observed_at: oldest,
            source_version: "test".to_owned(),
        },
        PoolEvidenceSourceStamp {
            source: PoolEvidenceSource::RepositoryForkPolicyRest,
            observed_at: now,
            source_version: "test".to_owned(),
        },
    ];
    let image_deadline = now + Duration::from_secs(600);

    assert_eq!(
        evidence_expiry(&sources, now, image_deadline, now),
        Ok(oldest + Duration::from_secs(30))
    );
}

#[test]
fn proof_expiry_keeps_stale_or_incomplete_evidence_unknown() {
    let now = UNIX_EPOCH + Duration::from_secs(10_000);
    let stale = [PoolEvidenceSourceStamp {
        source: PoolEvidenceSource::RepositoryMetadataRest,
        observed_at: now - Duration::from_secs(31),
        source_version: "test".to_owned(),
    }];
    assert_eq!(
        evidence_expiry(&stale, now, now + Duration::from_secs(600), now),
        Err(PoolAdmissionEvidence::Unknown(PolicyGap::StaleEvidence))
    );
    assert_eq!(
        evidence_expiry(&[], now, now + Duration::from_secs(600), now),
        Err(PoolAdmissionEvidence::Unknown(
            PolicyGap::IncompletePolicyRead
        ))
    );
}

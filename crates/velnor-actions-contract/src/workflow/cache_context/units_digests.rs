// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Group-ledger producer digest constructors for [`UnitLedgerV1`].

use crate::canonical::{Digest, canonical_json_bytes, digest_b3_typed};
use crate::errors::ContractError;

use super::units::{GroupDigestInputs, UnitLedgerV1};

impl UnitLedgerV1 {
    /// Canonical plan.json bytes digest: `b3(canonical plan.json)`.
    ///
    /// The caller passes the canonical bytes of the downloaded plan document
    /// (the byte form produced by `plan_json_bytes` / staged publication),
    /// and the digest is the raw BLAKE3 hash of those bytes.
    #[must_use]
    pub fn plan_digest_from_canonical_bytes(bytes: &[u8]) -> Digest {
        digest_b3_typed(bytes)
    }

    /// Group digest: `b3(canonical (group_key, plan_digest))`.
    ///
    /// `group_key` is the executing job key with the `-read`/`-write`
    /// branch suffix removed; `plan_digest` is the
    /// [`UnitLedgerV1::plan_digest_from_canonical_bytes`] digest of the same
    /// plan the ledger was emitted against.
    pub fn group_digest_for_group(
        group_key: &str,
        plan_digest: &Digest,
    ) -> Result<Digest, ContractError> {
        if group_key.is_empty() {
            return Err(ContractError::identity(
                "group_ledger.group_key",
                "unknown_required_identity",
            ));
        }
        let inputs = GroupDigestInputs {
            group_key,
            plan_digest,
        };
        Ok(digest_b3_typed(&canonical_json_bytes(&inputs)?))
    }

    /// Specified absence encoding covered per unit by
    /// [`UnitLedgerV1::observation_digest_for_sidecars`] when no observation
    /// sidecar was staged for that unit.
    pub const ABSENT_OBSERVATION_SIDECAR_INPUT: &str = "velnor-gc1/no-observation-sidecar/v1";

    /// Specified absence encoding covered per unit by
    /// [`UnitLedgerV1::source_binding_digest_for_sidecars`] when no
    /// source-binding sidecar was staged for that unit. No producer stages
    /// source-binding sidecars today, so this encoding is what honest emit
    /// produces until one does.
    pub const ABSENT_SOURCE_BINDING_SIDECAR_INPUT: &str = "velnor-gc1/no-source-binding-sidecar/v1";

    /// Combined sidecar digest over the ordered units: each unit contributes
    /// the BLAKE3 hash of its staged sidecar bytes (already canonical) or,
    /// when the unit staged none, of the caller-supplied absence-encoding
    /// constant; the set digest is the BLAKE3 hash of the canonical JSON
    /// array of those per-unit digest strings, in unit order.
    pub fn sidecar_set_digest(
        sidecars: &[Option<&[u8]>],
        absent_input: &str,
    ) -> Result<Digest, ContractError> {
        if absent_input.is_empty() {
            return Err(ContractError::identity(
                "group_ledger.absent_sidecar_input",
                "unknown_required_identity",
            ));
        }
        let mut per_unit = Vec::with_capacity(sidecars.len());
        for sidecar in sidecars {
            let input = sidecar.unwrap_or_else(|| absent_input.as_bytes());
            per_unit.push(digest_b3_typed(input).as_str().to_owned());
        }
        Ok(digest_b3_typed(&canonical_json_bytes(&per_unit)?))
    }

    /// Observation digest from staged unit-observation sidecars, in unit
    /// order: `sidecar_set_digest` with the
    /// [`UnitLedgerV1::ABSENT_OBSERVATION_SIDECAR_INPUT`] absence encoding.
    pub fn observation_digest_for_sidecars(
        sidecars: &[Option<&[u8]>],
    ) -> Result<Digest, ContractError> {
        Self::sidecar_set_digest(sidecars, Self::ABSENT_OBSERVATION_SIDECAR_INPUT)
    }

    /// Source-binding digest from staged source-binding sidecars, in unit
    /// order: `sidecar_set_digest` with the
    /// [`UnitLedgerV1::ABSENT_SOURCE_BINDING_SIDECAR_INPUT`] absence
    /// encoding.
    pub fn source_binding_digest_for_sidecars(
        sidecars: &[Option<&[u8]>],
    ) -> Result<Digest, ContractError> {
        Self::sidecar_set_digest(sidecars, Self::ABSENT_SOURCE_BINDING_SIDECAR_INPUT)
    }
}

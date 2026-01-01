//! Observed restore evidence: real observations, never bare bools.
//!
//! Every restore verdict derives from observed entry paths, bytes, and
//! digests compared against recorded expectations. No constructor takes
//! bare bools, so no caller can bless a restore without evidence. This
//! module also owns the single P04 zero-byte rule both cache layers
//! delegate to, so they can never disagree about empty outputs.

use velnor_actions_contract::validate_digest;

use crate::cache::verify_artifact_digest;

/// Observed restore evidence for one cache entry.
///
/// Each check compares an observed value against its recorded
/// expectation; [`RestoreObservation::checks`] reports the five outcomes
/// in [`RestoreCheck`](crate::restore::RestoreCheck) order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreObservation {
    /// Observed entry path; empty means no entry was restored.
    pub entry_path: String,
    /// Observed entry bytes.
    pub entry_bytes: Vec<u8>,
    /// Recorded entry digest the bytes must match.
    pub expected_digest: String,
    /// Recorded compatibility id.
    pub expected_compat: String,
    /// Observed compatibility id.
    pub observed_compat: String,
    /// Recorded owner trust scope.
    pub expected_owner: String,
    /// Observed owner trust scope.
    pub observed_owner: String,
    /// Recorded input digest.
    pub expected_inputs: String,
    /// Observed input digest.
    pub observed_inputs: String,
}

impl RestoreObservation {
    /// Real comparisons in check order: presence, digest, compat, owner,
    /// inputs. Presence needs a restored path; digests hash the observed
    /// bytes or compare well-formed recorded values, never bare flags.
    #[must_use]
    pub fn checks(&self) -> [bool; 5] {
        let present = !self.entry_path.trim().is_empty();
        let digest_ok = verify_artifact_digest(&self.entry_bytes, &self.expected_digest).is_ok();
        let compat_ok = digests_equal(&self.expected_compat, &self.observed_compat);
        let owner_ok =
            !self.observed_owner.is_empty() && self.observed_owner == self.expected_owner;
        let inputs_ok = digests_equal(&self.expected_inputs, &self.observed_inputs);
        [present, digest_ok, compat_ok, owner_ok, inputs_ok]
    }
}

/// Both digests well-formed and equal; malformed values never match.
fn digests_equal(expected: &str, observed: &str) -> bool {
    validate_digest(expected).is_ok() && expected == observed
}

/// Model-only 5-check chain for one provider-cache restore.
///
/// Pins the intended semantics (present, digest, compatibility,
/// trust, inputs): a miss would discard the entry for refetch, and a
/// hit would never disable verification. No production path consumes
/// `RestoreObservation` yet (task reports carry no restore
/// observations), so this classifies caller-built models only; the
/// live chain is exact-key restore, then lock-verified readonly init,
/// then mandatory validate. See `decisions.rs`.
/// # Errors
pub fn verify_provider_restore(obs: &RestoreObservation) -> Result<(), &'static str> {
    classify_restore(obs)
}

/// Classify a restore attempt from observed evidence: hit or precise reason.
///
/// Checks run in order: present, digest, compatibility, trust, inputs.
/// # Errors
pub fn classify_restore(obs: &RestoreObservation) -> Result<(), &'static str> {
    let reasons = [
        "no_entry",
        "cache_corrupt",
        "compatibility_mismatch",
        "trust_scope_mismatch",
        "input_digest_mismatch",
    ];
    for (index, ok) in obs.checks().iter().enumerate() {
        if !ok {
            return Err(reasons[index]);
        }
    }
    Ok(())
}

/// P04 zero-byte rule: a declared output with zero observed bytes is
/// incomplete evidence, never a verified reuse.
///
/// Single typed rule owned here; both the mise layer and the
/// orchestrator layer delegate to it, so they can never disagree.
#[must_use]
pub fn output_bytes_complete(bytes: &[u8]) -> bool {
    !bytes.is_empty()
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::ExitCode;

use velnor_actions_orchestrator::EntryTupleV1;
use velnor_actions_orchestrator::cache_publication::{
    SaveHalt, SaveOutcome, SaveRequest, decide_save,
};
use velnor_actions_orchestrator::cache_recovery::{
    ReadbackInputs, ReadbackVerdict, decide_readback,
};
use velnor_actions_orchestrator::{
    AdmissionDecision, InventoryV1, inventory_digest, parse_admission_bytes, parse_inventory_bytes,
};

use super::archive::{list_archive_members, payload_bytes, reread_payload, screen_archive_members};
use super::request::parse_expected;
use super::{
    Expected, STATE_TRANSPORT_REL, ValidatedCacheRequest, find_entry, observed_entry, single_line,
};
use crate::dispatch::fail_internal;

struct PublishPlan {
    inventory: InventoryV1,
    expected: Expected,
    ceiling: u64,
    bytes: Vec<u8>,
}

fn admit_publish(
    request: &ValidatedCacheRequest,
    runner_temp: &Path,
) -> Result<PublishPlan, String> {
    let admission = match parse_admission_bytes(&request.admission_bytes) {
        Ok(admission) => admission,
        Err(error) => return Err(single_line(&error.to_string())),
    };
    if admission.decision != AdmissionDecision::Admitted {
        return Err(format!(
            "cache publish blocked: {}",
            admission.blocked_reasons.join(",")
        ));
    }
    let inventory = match parse_inventory_bytes(&request.inventory_bytes) {
        Ok(inventory) => inventory,
        Err(error) => return Err(single_line(&error.to_string())),
    };
    let computed = match inventory_digest(&inventory) {
        Ok(computed) => computed,
        Err(error) => return Err(single_line(&error.to_string())),
    };
    let expected = parse_expected(&request.expected_bytes)?;
    // Bind the admission to the request it authorizes: a stale admission
    // carries obsolete headroom and disk measurements, and a
    // cross-generation admission authorizes a different candidate, so both
    // bindings deny publication, following the decide_* denial shape.
    if computed != admission.inventory_digest {
        return Err("cache publish denied: inventory_digest_mismatch".to_owned());
    }
    if admission.generation.cache_key() != expected.key {
        return Err("cache publish denied: generation_key_mismatch".to_owned());
    }
    let ceiling = if admission.operating_ceiling_bytes != 0 {
        admission.operating_ceiling_bytes
    } else {
        admission.hard_ceiling_bytes
    };
    let payload = runner_temp.join(STATE_TRANSPORT_REL);
    let bytes = payload_bytes(&payload)?;
    Ok(PublishPlan {
        inventory,
        expected,
        ceiling,
        bytes,
    })
}

fn admit_save(plan: &PublishPlan) -> Result<Option<EntryTupleV1>, String> {
    let len = u64::try_from(plan.bytes.len()).unwrap_or(u64::MAX);
    let exact = find_entry(&plan.inventory, &plan.expected)
        .filter(|entry| entry.size_in_bytes == len)
        .cloned();
    let save = SaveRequest {
        admission_admitted: true,
        candidate_complete: true,
        exact_usable_exists: exact.is_some(),
        denied_reason: None,
        candidate_bytes: Some(len),
        ceiling_bytes: plan.ceiling,
        save_halt: SaveHalt::Running,
    };
    match decide_save(&save) {
        Ok(SaveOutcome::Attempted | SaveOutcome::SkippedExisting) => Ok(exact),
        Ok(outcome) => Err(format!("cache publish {outcome:?}")),
        Err(error) => Err(single_line(&error.to_string())),
    }
}

fn report_publish(
    plan: &PublishPlan,
    entry: Option<&EntryTupleV1>,
    reread: &[u8],
    digest: &str,
) -> ExitCode {
    let len = u64::try_from(plan.bytes.len()).unwrap_or(u64::MAX);
    let Some(entry) = entry else {
        println!(
            "velnor: state snapshot published ({len} bytes, sha256:{digest}, exact-readback, no prior entry)"
        );
        return ExitCode::SUCCESS;
    };
    let observed = observed_entry(entry);
    let inputs = ReadbackInputs {
        frozen_archive_digest: digest,
        frozen_archive_bytes: len,
        restored_bytes: Some(reread),
        inventory_complete: plan.inventory.complete,
        observed_entry: Some(&observed),
        expected_key: &plan.expected.key,
        expected_ref: &plan.expected.ref_name,
        expected_version: &plan.expected.version,
    };
    match decide_readback(&inputs) {
        Ok(ReadbackVerdict::Verified {
            archive_digest,
            stored_bytes,
        }) => {
            println!(
                "velnor: state snapshot published ({stored_bytes} bytes, sha256:{archive_digest})"
            );
            ExitCode::SUCCESS
        }
        Ok(ReadbackVerdict::Corrupt { evidence }) => {
            fail_internal(&format!("cache publish readback_corrupt: {evidence}"))
        }
        Ok(ReadbackVerdict::Unavailable { reason }) => {
            fail_internal(&format!("cache publish readback_unavailable: {reason}"))
        }
        Err(error) => fail_internal(&single_line(&error.to_string())),
    }
}

pub(super) fn run_publish(request: &ValidatedCacheRequest, runner_temp: &Path) -> ExitCode {
    let plan = match admit_publish(request, runner_temp) {
        Ok(plan) => plan,
        Err(problem) => return fail_internal(&problem),
    };
    let entry = match admit_save(&plan) {
        Ok(entry) => entry,
        Err(problem) => return fail_internal(&problem),
    };
    let payload = runner_temp.join(STATE_TRANSPORT_REL);
    let members = match list_archive_members(&payload) {
        Ok(members) => members,
        Err(problem) => return fail_internal(&problem),
    };
    if let Err(problem) = screen_archive_members(&members) {
        return fail_internal(&problem);
    }
    let (reread, digest) = match reread_payload(&payload, &plan.bytes, "publish") {
        Ok(pair) => pair,
        Err(problem) => return fail_internal(&problem),
    };
    report_publish(&plan, entry.as_ref(), &reread, &digest)
}

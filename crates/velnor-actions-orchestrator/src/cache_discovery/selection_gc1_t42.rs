// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::canonical::{Digest, digest_b3, digest_b3_typed};
use velnor_actions_contract::workflow::cache_context::JobBindingV1;
use velnor_actions_contract::workflow::cache_identity::{Gc1Family, GenerationId};

use super::super::types::{DiscoveryRequest, EntryProvenance, SelectableEntry};
use super::select_predecessor;

fn fixture_job() -> Result<JobBindingV1, Box<dyn std::error::Error>> {
    Ok(JobBindingV1 {
        schema: 1,
        repository_id: 1,
        repository: "tailrocks/velnor-new".to_owned(),
        run_id: 7,
        run_attempt: 1,
        job_id: 11,
        job_key: "writer".to_owned(),
        event: "push".to_owned(),
        ref_name: "refs/heads/main".to_owned(),
        workflow_sha: format!("{:040x}", 0xa5a5),
        source_sha: format!("{:040x}", 0x5a5a),
        plan_digest: Digest::parse(&digest_b3(b"gc1-t42-plan"))?,
    })
}

fn fixture_generation() -> Result<GenerationId, Box<dyn std::error::Error>> {
    Ok(GenerationId {
        family: Gc1Family::State,
        group_digest: Digest::parse(&digest_b3(b"gc1-t42-group"))?,
        compatibility_digest: Digest::parse(&digest_b3(b"gc1-t42-compat"))?,
        coverage_digest: Digest::parse(&digest_b3(b"gc1-t42-coverage"))?,
        repair_revision: 0,
    })
}

fn predecessor_generation(seed: &[u8]) -> Result<GenerationId, Box<dyn std::error::Error>> {
    let base = fixture_generation()?;
    Ok(GenerationId {
        coverage_digest: Digest::parse(&digest_b3(seed))?,
        ..base
    })
}

fn entry(cache_id: u64, key: &str, created_at: &str) -> SelectableEntry {
    SelectableEntry {
        cache_id,
        repository_id: 1,
        ref_name: "refs/heads/main".to_owned(),
        key: key.to_owned(),
        version: "v1".to_owned(),
        size_in_bytes: 512,
        created_at: created_at.to_owned(),
        provenance: EntryProvenance::Trusted,
    }
}

fn discovery_request(
    generation: GenerationId,
    entries: Vec<SelectableEntry>,
) -> Result<DiscoveryRequest, Box<dyn std::error::Error>> {
    Ok(DiscoveryRequest {
        job: fixture_job()?,
        generation,
        expected_cache_version: "v1".to_owned(),
        allowed_refs: vec!["refs/heads/main".to_owned()],
        current_ref: "refs/heads/main".to_owned(),
        inventory_digest: digest_b3_typed(b"gc1-t42-inventory"),
        inventory_complete: true,
        entries,
        active_coverage_ids: Vec::new(),
    })
}

#[test]
fn gc1_t42_selects_newest_compatible_predecessor() -> Result<(), Box<dyn std::error::Error>> {
    let generation = fixture_generation()?;
    let older = predecessor_generation(b"gc1-t42-older-coverage")?;
    let newer = predecessor_generation(b"gc1-t42-newer-coverage")?;
    let request = discovery_request(
        generation,
        vec![
            entry(7, &older.cache_key(), "2026-10-20T00:00:00Z"),
            entry(9, &newer.cache_key(), "2026-10-24T00:00:00Z"),
        ],
    )?;
    let visible: Vec<&SelectableEntry> = request.entries.iter().collect();
    let predecessor = select_predecessor(&request, &visible)
        .ok_or_else(|| std::io::Error::other("compatible predecessor missed selection"))?;
    if predecessor.entry.cache_id != 9 {
        return Err(std::io::Error::other(
            "predecessor selection kept the oldest entry instead of the newest",
        )
        .into());
    }
    Ok(())
}

#[test]
fn gc1_t42_prefers_larger_cache_id_on_timestamp_tie() -> Result<(), Box<dyn std::error::Error>> {
    let generation = fixture_generation()?;
    let first = predecessor_generation(b"gc1-t42-tie-a-coverage")?;
    let second = predecessor_generation(b"gc1-t42-tie-b-coverage")?;
    let request = discovery_request(
        generation,
        vec![
            entry(7, &first.cache_key(), "2026-10-22T00:00:00Z"),
            entry(9, &second.cache_key(), "2026-10-22T00:00:00Z"),
        ],
    )?;
    let visible: Vec<&SelectableEntry> = request.entries.iter().collect();
    let predecessor = select_predecessor(&request, &visible)
        .ok_or_else(|| std::io::Error::other("tied predecessor missed selection"))?;
    if predecessor.entry.cache_id != 9 {
        return Err(std::io::Error::other(
            "timestamp tie-break kept the smaller cache id instead of the larger",
        )
        .into());
    }
    Ok(())
}

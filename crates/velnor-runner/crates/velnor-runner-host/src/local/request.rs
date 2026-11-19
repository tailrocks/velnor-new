// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;

use crate::config::RunnerPlatform;
use crate::error::HostError;
use crate::worker::{ResourceBudget, ResourceBudgetConfig, WorkerIdentityNames};

use super::storage::Sizes;

const OPERATIONAL_RESERVE_BYTES: u64 = 1024 * 1024 * 1024;
const RESERVED_NETWORKS: [&str; 4] = ["bridge", "host", "none", "default"];
const LOCAL_NETWORK_PREFIX: &str = "velnor-local-";
const PRODUCTION_NETWORK_PREFIX: &str = "velnor-";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalPairRequest {
    pub environment: String,
    pub side: String,
    pub agent: String,
    pub network: String,
    pub platform: String,
    pub runner_image: String,
    pub dind_image: String,
    pub sequence: u32,
    pub runner_cpu_millicores: u64,
    pub runner_memory_bytes: u64,
    pub dind_cpu_millicores: u64,
    pub dind_memory_bytes: u64,
    pub home_bytes: u64,
    pub work_bytes: u64,
    pub tmp_bytes: u64,
}

pub(super) struct Validated {
    pub(super) platform: RunnerPlatform,
    pub(super) budget: ResourceBudget,
    pub(super) names: WorkerIdentityNames,
    pub(super) pair: String,
    pub(super) sizes: Sizes,
}

pub(super) fn validate(request: &LocalPairRequest) -> Result<Validated, HostError> {
    if !valid_environment(&request.environment)
        || !matches!(request.side.as_str(), "trusted" | "untrusted")
        || !valid_agent(&request.agent)
        || !valid_network(&request.network)
        || !image_id(&request.runner_image)
        || !image_id(&request.dind_image)
        || request.sequence == 0
    {
        return Err(HostError::Config);
    }
    let platform = RunnerPlatform::parse(&request.platform).ok_or(HostError::Config)?;
    let budget = ResourceBudgetConfig {
        runner_cpu_millicores: request.runner_cpu_millicores,
        runner_memory_bytes: request.runner_memory_bytes,
        dind_cpu_millicores: request.dind_cpu_millicores,
        dind_memory_bytes: request.dind_memory_bytes,
        operational_memory_reserve_bytes: OPERATIONAL_RESERVE_BYTES,
    }
    .validate()?;
    let sizes = Sizes {
        home: request.home_bytes,
        work: request.work_bytes,
        tmp: request.tmp_bytes,
    }
    .checked()?;
    let mut nonce = [0_u8; 16];
    getrandom::fill(&mut nonce).map_err(|_| HostError::Identity)?;
    let names = WorkerIdentityNames::from_nonce(&crate::digest_hex::lowercase_hex(&nonce))?;
    let pair = names.private_volume().to_owned();
    Ok(Validated {
        platform,
        budget,
        names,
        pair,
        sizes,
    })
}

fn valid_environment(value: &str) -> bool {
    (1..=48).contains(&value.len())
        && value.starts_with(|first: char| first.is_ascii_lowercase() || first.is_ascii_digit())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn valid_agent(value: &str) -> bool {
    (1..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn valid_network(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    (1..=64).contains(&value.len())
        && !RESERVED_NETWORKS.contains(&lower.as_str())
        && !lower.bytes().all(|byte| byte.is_ascii_hexdigit())
        && (!lower.starts_with(PRODUCTION_NETWORK_PREFIX)
            || lower.starts_with(LOCAL_NETWORK_PREFIX))
        && value.starts_with(|first: char| first.is_ascii_alphanumeric())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

fn image_id(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

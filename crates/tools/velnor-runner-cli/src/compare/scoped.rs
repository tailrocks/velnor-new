//! Offline command adapter for a versioned, scope-bound comparison input.
//!
//! The serialized provider result is caller-supplied data. A `Complete` tag
//! does not authenticate GitHub or prove the workflow producer graph; success
//! therefore reports only `BOUND_ONLY` identities.

use std::fmt::{self, Formatter};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::ExitCode;

use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use velnor_actions_contract_workflow::{Plan, TaskRuntimeReceipt};
use velnor_actions_orchestrator_merge_ports::{
    ScopedCompareRequest, ScopedCompareResult, TaskReportOutputFanIn, bind_scoped_compare,
};
use velnor_runner_github::{
    ActionsWorkflowAttemptEvidenceGap, ActionsWorkflowAttemptProviderEvidence,
    ActionsWorkflowAttemptProviderRead,
};

use super::{ActionsAttemptAdapterOutcome, adapt_actions_attempt_read};

const MAX_INPUT_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScopedCompareEvidence {
    schema: u32,
    plan: Plan,
    receipts: Vec<TaskRuntimeReceipt>,
    producer_outputs: TaskReportOutputFanIn,
    provider_read: ProviderReadWire,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderReadWire {
    kind: String,
    value: Value,
}

/// A JSON value which rejects duplicate keys at every nesting level.
///
/// Parsing directly into `serde_json::Value` would silently keep only one
/// duplicate object member before the typed DTO parser can reject ambiguity.
struct UniqueJson(Value);

impl<'de> Deserialize<'de> for UniqueJson {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(UniqueJsonVisitor)
    }
}

struct UniqueJsonVisitor;

impl<'de> Visitor<'de> for UniqueJsonVisitor {
    type Value = UniqueJson;

    fn expecting(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("JSON without duplicate object keys")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(UniqueJson(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(UniqueJson(Value::from(value)))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(UniqueJson(Value::from(value)))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        serde_json::Number::from_f64(value)
            .map(Value::Number)
            .map(UniqueJson)
            .ok_or_else(|| E::custom("non-finite JSON number"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(UniqueJson(Value::String(value.to_owned())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(UniqueJson(Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(UniqueJson(Value::Null))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(UniqueJson(Value::Null))
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(UniqueJson(value)) = sequence.next_element()? {
            values.push(value);
        }
        Ok(UniqueJson(Value::Array(values)))
    }

    fn visit_map<A>(self, mut object: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = serde_json::Map::new();
        while let Some(key) = object.next_key::<String>()? {
            if values.contains_key(&key) {
                return Err(de::Error::custom("duplicate JSON object key"));
            }
            let UniqueJson(value) = object.next_value()?;
            values.insert(key, value);
        }
        Ok(UniqueJson(Value::Object(values)))
    }
}

impl ProviderReadWire {
    fn into_read(self) -> Result<ActionsWorkflowAttemptProviderRead, ()> {
        match self.kind.as_str() {
            "complete" => {
                serde_json::from_value::<ActionsWorkflowAttemptProviderEvidence>(self.value)
                    .map(Box::new)
                    .map(ActionsWorkflowAttemptProviderRead::Complete)
                    .map_err(|_| ())
            }
            "unavailable" => {
                serde_json::from_value::<ActionsWorkflowAttemptEvidenceGap>(self.value)
                    .map(ActionsWorkflowAttemptProviderRead::Unavailable)
                    .map_err(|_| ())
            }
            _ => Err(()),
        }
    }
}

/// Parse and bind one offline evidence file for the command's exact scope.
///
/// This function deliberately does not call the GitHub API. The input must be
/// the retained output of an upstream bounded reader, but this file format is
/// forgeable data and cannot establish that provenance by itself.
pub(crate) fn compare_scoped_file_for(
    path: &Path,
    repository: &str,
    run_id: u64,
    attempt: u64,
) -> ExitCode {
    if let Ok(result) = compare_scoped_file(path, repository, run_id, attempt) {
        println!(
            "BOUND_ONLY lanes={} repository={} run_id={} attempt={}",
            result.lanes.len(),
            result.repository,
            result.run_id,
            result.attempt
        );
        ExitCode::SUCCESS
    } else {
        println!("NOT_PROVEN");
        eprintln!("scoped evidence is missing, invalid, incomplete, or mismatched");
        ExitCode::from(1)
    }
}

fn compare_scoped_file(
    path: &Path,
    repository: &str,
    run_id: u64,
    attempt: u64,
) -> Result<ScopedCompareResult, ()> {
    let run_id = i64::try_from(run_id).map_err(|_| ())?;
    let attempt = u32::try_from(attempt).map_err(|_| ())?;
    if !super::valid_repository(repository) || run_id <= 0 || attempt == 0 {
        return Err(());
    }

    let evidence = read_evidence(path)?;
    if evidence.schema != 1 {
        return Err(());
    }
    let request = ScopedCompareRequest {
        repository,
        run_id,
        attempt,
    };
    let provider = match adapt_actions_attempt_read(
        evidence.provider_read.into_read()?,
        request,
        &evidence.plan.head,
    )
    .map_err(|_| ())?
    {
        ActionsAttemptAdapterOutcome::Complete(provider) => provider,
        ActionsAttemptAdapterOutcome::Unavailable(_) => return Err(()),
    };
    bind_scoped_compare(
        request,
        &evidence.plan,
        &evidence.receipts,
        &evidence.producer_outputs,
        &provider,
    )
    .map_err(|_| ())
}

fn read_evidence(path: &Path) -> Result<ScopedCompareEvidence, ()> {
    let file = File::open(path).map_err(|_| ())?;
    if !file.metadata().map_err(|_| ())?.is_file() {
        return Err(());
    }
    let mut bytes = Vec::new();
    file.take(MAX_INPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    if u64::try_from(bytes.len()).map_err(|_| ())? > MAX_INPUT_BYTES {
        return Err(());
    }
    let mut deserializer = serde_json::Deserializer::from_slice(&bytes);
    let UniqueJson(value) = UniqueJson::deserialize(&mut deserializer).map_err(|_| ())?;
    deserializer.end().map_err(|_| ())?;
    serde_json::from_value(value).map_err(|_| ())
}

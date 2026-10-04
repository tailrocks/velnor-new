//! Compiler-only chunk transport; source and argument bytes never enlarge `run:`.
use crate::RenderError;
use std::collections::BTreeMap;
use velnor_actions_contract::CompiledSourceHelper;

pub(super) const PREFIX: &str = "VELNOR_COMPILED_HELPER_";
const CHUNK_SIZE: usize = 8192;
const AMBIENT_RESERVE: usize = 65_536;
const RUSTUP_POLICY: &str = "RUSTUP_AUTO_INSTALL=0";

pub(super) fn encode(
    record: &CompiledSourceHelper,
    environment: &mut BTreeMap<String, String>,
    runs_on: &str,
) -> Result<String, RenderError> {
    validate_runner(runs_on)?;
    validate_raw_arguments(record)?;
    let source = record.source().as_bytes();
    let descriptor = record.invocation().descriptor();
    if velnor_actions_contract::compiled_source_sha256(source) != descriptor.source_sha256() {
        return Err(invalid("source_digest"));
    }
    let arguments = serde_json::to_vec(record.invocation().args())
        .map_err(|error| invalid(&error.to_string()))?;
    let prefix = execution_prefix(record)?;
    let execution = serde_json::to_vec(&prefix).map_err(|error| invalid(&error.to_string()))?;
    validate_vectors(record, &prefix, &arguments, &execution, runs_on)?;
    environment.insert(format!("{PREFIX}SCHEMA"), "1".to_owned());
    chunks(environment, "SOURCE", source)?;
    chunks(environment, "ARGUMENTS", &arguments)?;
    chunks(environment, "EXECUTION", &execution)?;
    let run = launcher(record, &arguments, &execution)?;
    if run.chars().count() > 21_000 {
        return Err(budget("run_size", run.chars().count(), 21_000));
    }
    validate_budget(record, environment, runs_on, &run)?;
    Ok(run)
}

pub(super) fn validate_runner(runs_on: &str) -> Result<(), RenderError> {
    target_limit(runs_on).map(|_| ())
}

pub(super) fn validate_raw_arguments(record: &CompiledSourceHelper) -> Result<(), RenderError> {
    let measured = record
        .invocation()
        .args()
        .iter()
        .map(String::len)
        .sum::<usize>();
    let limit = velnor_actions_contract::SOURCE_HELPER_ARGUMENT_BYTES_MAX;
    if measured > limit {
        return Err(budget("raw_argument_size", measured, limit));
    }
    Ok(())
}

fn validate_vectors(
    record: &CompiledSourceHelper,
    prefix: &[String],
    arguments: &[u8],
    execution: &[u8],
    runs_on: &str,
) -> Result<(), RenderError> {
    let prefix_bytes = prefix.iter().map(String::len).sum::<usize>();
    if prefix.len() > 2048 {
        return Err(budget("execution_count", prefix.len(), 2048));
    }
    if prefix_bytes > velnor_actions_contract::SOURCE_HELPER_ARGUMENT_BYTES_MAX {
        return Err(budget(
            "execution_size",
            prefix_bytes,
            velnor_actions_contract::SOURCE_HELPER_ARGUMENT_BYTES_MAX,
        ));
    }
    if runs_on.starts_with("ubuntu-")
        && record
            .invocation()
            .args()
            .iter()
            .chain(record.invocation().execution_prefix())
            .any(|argument| argument.len() >= 131_072)
    {
        let measured = record
            .invocation()
            .args()
            .iter()
            .chain(record.invocation().execution_prefix())
            .map(|argument| argument.len() + 1)
            .max()
            .unwrap_or_default();
        return Err(budget(
            "unsupported_argument_string_limit",
            measured,
            131_072,
        ));
    }
    if arguments.len() > 1_048_576 || execution.len() > 1_048_576 {
        return Err(budget(
            "arguments_size",
            arguments.len().max(execution.len()),
            1_048_576,
        ));
    }
    Ok(())
}

fn launcher(
    record: &CompiledSourceHelper,
    arguments: &[u8],
    execution: &[u8],
) -> Result<String, RenderError> {
    let source = record.source().as_bytes();
    let descriptor = record.invocation().descriptor();
    let digest = velnor_actions_contract::compiled_source_sha256(arguments);
    let mut bindings = record.execution_recipe().map_or_else(Vec::new, |_| {
        record.environment().keys().collect::<Vec<_>>()
    });
    let output_key = "GITHUB_OUTPUT".to_owned();
    if record.github_output() {
        bindings.push(&output_key);
    }
    let bindings = serde_json::to_string(&bindings).map_err(|error| invalid(&error.to_string()))?;
    let python = format!(
        "{}\n{}\nsource, arguments, prefix = decode_transport(os.environ, '{}', {}, '{}', {}, '{}', {}, {})\nfor key in tuple(os.environ):\n    if key.startswith(TRANSPORT_PREFIX):\n        del os.environ[key]\nraise SystemExit(execute(source, '{}', arguments, prefix))\n",
        include_str!("source_helper_transport.py"),
        include_str!("source_helper_launcher.py"),
        descriptor.source_sha256(),
        source.len(),
        digest,
        arguments.len(),
        velnor_actions_contract::compiled_source_sha256(execution),
        execution.len(),
        bindings,
        descriptor.source_sha256()
    );
    Ok(format!(
        "/usr/bin/python3 -I -S - <<'VELNOR_QUALIFIED_SOURCE_HELPER'\n{python}VELNOR_QUALIFIED_SOURCE_HELPER"
    ))
}

fn validate_budget(
    record: &CompiledSourceHelper,
    environment: &BTreeMap<String, String>,
    runs_on: &str,
    run: &str,
) -> Result<(), RenderError> {
    let environment_bytes: usize = environment
        .iter()
        .map(|(key, value)| key.len() + value.len() + 2)
        .sum();
    let pointers = 8
        * (environment.len()
            + record.invocation().args().len()
            + record.invocation().execution_prefix().len()
            + usize::from(record.execution_recipe().is_some())
            + record
                .execution_recipe()
                .map_or(0, |_| record.environment().len())
            + 16);
    if runs_on.starts_with("ubuntu-")
        && environment
            .iter()
            .any(|(key, value)| key.len() + value.len() + 2 > 131_072)
    {
        let measured = environment
            .iter()
            .map(|(key, value)| key.len() + value.len() + 2)
            .max()
            .unwrap_or_default();
        return Err(budget(
            "unsupported_environment_string_limit",
            measured,
            131_072,
        ));
    }
    let mut child_bytes: usize = environment
        .iter()
        .filter(|(key, _)| !key.starts_with(PREFIX))
        .map(|(key, value)| key.len() + value.len() + 2)
        .chain(
            record
                .invocation()
                .args()
                .iter()
                .chain(record.invocation().execution_prefix())
                .map(|value| value.len() + 1),
        )
        .sum();
    if record.execution_recipe().is_some() {
        child_bytes += RUSTUP_POLICY.len() + 1;
        child_bytes += record
            .environment()
            .iter()
            .map(|(key, value)| key.len() + value.len() + 2)
            .sum::<usize>();
    }
    let measured = (environment_bytes + run.len()).max(child_bytes) + pointers + AMBIENT_RESERVE;
    let limit = target_limit(runs_on)?;
    if measured >= limit {
        return Err(budget("unsupported_environment_limit", measured, limit));
    }
    Ok(())
}

fn target_limit(runs_on: &str) -> Result<usize, RenderError> {
    match runs_on {
        "ubuntu-22.04" | "ubuntu-24.04" | "ubuntu-24.04-arm" | "ubuntu-26.04"
        | "ubuntu-26.04-arm" => Ok(2_097_152),
        // Apple's macOS 26 release XNU12377.1.9 bsd/sys/syslimits.h: ARG_MAX=1MiB.
        "macos-26" => Ok(1_048_576),
        _ => Err(invalid("unsupported_runner")),
    }
}

fn execution_prefix(record: &CompiledSourceHelper) -> Result<Vec<String>, RenderError> {
    let prefix = record.invocation().execution_prefix();
    if record.execution_recipe().is_none() {
        return Ok(Vec::new());
    }
    if prefix.first().is_none_or(|value| value != "/usr/bin/env")
        || prefix.get(1).is_none_or(|value| value != "-i")
    {
        return Err(invalid("execution_environment_envelope"));
    }
    if record.environment().keys().any(|key| {
        key.as_bytes()
            .first()
            .is_none_or(|byte| !byte.is_ascii_uppercase() && *byte != b'_')
    }) {
        return Err(invalid("execution_binding_key"));
    }
    let mut bound = prefix[..2].to_vec();
    bound.push(RUSTUP_POLICY.to_owned());
    if record.github_output() {
        bound.push("GITHUB_OUTPUT=${GITHUB_OUTPUT}".to_owned());
    }
    bound.extend(
        record
            .environment()
            .keys()
            .map(|key| format!("{key}=${{{key}}}")),
    );
    bound.extend_from_slice(&prefix[2..]);
    Ok(bound)
}

fn chunks(
    environment: &mut BTreeMap<String, String>,
    name: &str,
    bytes: &[u8],
) -> Result<(), RenderError> {
    let encoded = base64(bytes)?;
    let count = encoded.len().div_ceil(CHUNK_SIZE);
    environment.insert(format!("{PREFIX}{name}_COUNT"), count.to_string());
    for (index, chunk) in encoded.as_bytes().chunks(CHUNK_SIZE).enumerate() {
        let value = std::str::from_utf8(chunk).map_err(|error| invalid(&error.to_string()))?;
        environment.insert(format!("{PREFIX}{name}_{index:04}"), value.to_owned());
    }
    Ok(())
}

fn base64(bytes: &[u8]) -> Result<String, RenderError> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let word = (chunk.first().map_or(0, |byte| u32::from(*byte)) << 16)
            | (chunk.get(1).map_or(0, |byte| u32::from(*byte)) << 8)
            | chunk.get(2).map_or(0, |byte| u32::from(*byte));
        for (position, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if position > chunk.len() {
                encoded.push('=');
                continue;
            }
            let index = ((word >> shift) & 63) as usize;
            let byte = ALPHABET.get(index).ok_or_else(|| invalid("base64_index"))?;
            encoded.push(char::from(*byte));
        }
    }
    Ok(encoded)
}

fn invalid(problem: &str) -> RenderError {
    RenderError::BadCommand(format!("source_helper_transport:{problem}"))
}

fn budget(reason: &'static str, measured: usize, limit: usize) -> RenderError {
    RenderError::UnsupportedHelperTransport(crate::source_helper_budget::TransportBudgetExceeded {
        reason,
        measured,
        limit,
    })
}

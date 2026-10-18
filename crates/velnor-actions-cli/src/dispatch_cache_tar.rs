use std::env;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use velnor_actions_orchestrator::cache_action_stage::{
    StageAccess, StageContext, StageReceiptClaim, run_cache_tar,
};

const OUTPUT_KEY: &str = "cache-stage-claim";
pub(crate) const OP: &str = "cache-action-stage-tar-v1";

/// Runs one pinned SDK tar operation through the trusted stage adapter.
pub(crate) fn run(args: &[OsString]) -> ExitCode {
    if args.len() == 1 && args[0] == "--version" {
        println!("tar (GNU tar) velnor-cache-adapter-v1");
        return ExitCode::SUCCESS;
    }
    if !super::cache_runner_context_is_valid() {
        return fail("cache_runner_context_invalid");
    }
    let outcome = run_with_env(args);
    match outcome {
        Ok(Some(claim)) => append_claim(&claim),
        Ok(None) => ExitCode::SUCCESS,
        Err(problem) => fail(problem),
    }
}

fn run_with_env(args: &[OsString]) -> Result<Option<String>, &'static str> {
    let runner_temp = required_path("RUNNER_TEMP")?;
    let workspace = required_path("GITHUB_WORKSPACE")?;
    let namespace = required_value("VELNOR_CACHE_STAGE_NAMESPACE")?;
    let key = required_value("VELNOR_CACHE_STAGE_KEY")?;
    let context = match namespace.as_str() {
        "normal-tools-v3" => {
            if env::var_os("VELNOR_CACHE_STAGE_CAMPAIGN").is_some()
                || env::var_os("VELNOR_CACHE_STAGE_LAYER").is_some()
            {
                return Err("cache_stage_context_invalid");
            }
            StageContext::normal(&runner_temp, &workspace, &key)?
        }
        "qualification-v3" => {
            let campaign = required_value("VELNOR_CACHE_STAGE_CAMPAIGN")?;
            let layer = required_value("VELNOR_CACHE_STAGE_LAYER")?;
            StageContext::qualification(&runner_temp, &workspace, &campaign, &layer, &key)?
        }
        _ => return Err("cache_stage_context_invalid"),
    };
    let access_text = required_value("VELNOR_CACHE_STAGE_ACCESS")?;
    let claim_text = optional_value("VELNOR_CACHE_STAGE_RECEIPT")?;
    let claim = claim_text
        .as_deref()
        .map(StageReceiptClaim::parse)
        .transpose()?;
    let access = match (access_text.as_str(), claim.as_ref()) {
        ("new", None) => StageAccess::New,
        ("read", Some(claim)) => StageAccess::Read(claim),
        ("rewrite", Some(claim)) => StageAccess::Rewrite(claim),
        _ => return Err("cache_stage_access_invalid"),
    };
    let zstd_path = optional_path("VELNOR_CACHE_ZSTD_PATH")?;
    let outcome = run_cache_tar(&context, args, access, zstd_path.as_deref())?;
    Ok(outcome.receipt.map(|receipt| receipt.encode()))
}

fn append_claim(claim: &str) -> ExitCode {
    if claim.len() > 128
        || claim
            .bytes()
            .any(|byte| matches!(byte, b'\n' | b'\r' | b'='))
    {
        return fail("cache_stage_claim_invalid");
    }
    let Some(path) = env::var_os("GITHUB_OUTPUT").filter(|value| !value.is_empty()) else {
        return fail("missing_github_output");
    };
    let mut body = String::with_capacity(OUTPUT_KEY.len() + claim.len() + 2);
    body.push_str(OUTPUT_KEY);
    body.push('=');
    body.push_str(claim);
    body.push('\n');
    match super::cache_bind::append_owned_output(&path, body.as_bytes()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => fail("append_github_output_failed"),
    }
}

fn required_path(name: &str) -> Result<PathBuf, &'static str> {
    let path = PathBuf::from(required_value(name)?);
    if !path.is_absolute()
        || path.as_os_str().len() > 4_096
        || path.to_str().is_none_or(|text| {
            text.split('/')
                .skip(1)
                .any(|part| part.is_empty() || matches!(part, "." | ".."))
        })
    {
        return Err("cache_stage_path_invalid");
    }
    Ok(path)
}

fn optional_path(name: &str) -> Result<Option<PathBuf>, &'static str> {
    let Some(value) = env::var_os(name).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let path = PathBuf::from(value);
    if !path.is_absolute() || path.as_os_str().len() > 4_096 {
        return Err("cache_tar_compression_unavailable");
    }
    Ok(Some(path))
}

fn required_value(name: &str) -> Result<String, &'static str> {
    let value = env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or("cache_stage_input_missing")?;
    if value.len() > 4_096 || value.chars().any(|ch| matches!(ch, '\n' | '\r' | '\0')) {
        return Err("cache_stage_input_invalid");
    }
    Ok(value)
}

fn optional_value(name: &str) -> Result<Option<String>, &'static str> {
    let Some(value) = env::var(name).ok().filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if value.len() > 256 || value.chars().any(|ch| matches!(ch, '\n' | '\r' | '\0')) {
        return Err("cache_stage_claim_invalid");
    }
    Ok(Some(value))
}

fn fail(problem: &str) -> ExitCode {
    eprintln!("velnor-actions: cache tar failed: {problem}");
    ExitCode::from(1)
}

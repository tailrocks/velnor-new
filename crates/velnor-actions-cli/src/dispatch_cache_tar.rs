use std::env;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use velnor_actions_orchestrator::cache_action_stage::{
    StageAccess, StageContext, StageReceiptClaim, TarOutcome, run_cache_tar,
};

const CLAIM_OUTPUT_KEY: &str = "cache-stage-claim";
const COMPRESSION_OUTPUT_KEY: &str = "cache-stage-compression";
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
        Ok(outcome) => append_outcome(&outcome),
        Err(problem) => fail(problem),
    }
}

fn run_with_env(args: &[OsString]) -> Result<TarOutcome, &'static str> {
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
        "normal-tofu-providers-payload-v3" => {
            if env::var_os("VELNOR_CACHE_STAGE_CAMPAIGN").is_some()
                || env::var_os("VELNOR_CACHE_STAGE_LAYER").is_some()
            {
                return Err("cache_stage_context_invalid");
            }
            StageContext::normal_tofu_providers(&runner_temp, &workspace, &key)?
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
    run_cache_tar(&context, args, access, zstd_path.as_deref())
}

fn append_outcome(outcome: &TarOutcome) -> ExitCode {
    let body = match outcome_body(outcome) {
        Ok(Some(body)) => body,
        Ok(None) => return ExitCode::SUCCESS,
        Err(problem) => return fail(problem),
    };
    let Some(path) = env::var_os("GITHUB_OUTPUT").filter(|value| !value.is_empty()) else {
        return fail("missing_github_output");
    };
    match super::cache_bind::append_owned_output(&path, body.as_bytes()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => fail("append_github_output_failed"),
    }
}

fn outcome_body(outcome: &TarOutcome) -> Result<Option<String>, &'static str> {
    if outcome.receipt.is_none() && outcome.compression.is_none() {
        return Ok(None);
    }
    let mut body = String::new();
    if let Some(receipt) = &outcome.receipt {
        let claim = receipt.encode();
        if claim.len() > 128
            || claim
                .bytes()
                .any(|byte| matches!(byte, b'\n' | b'\r' | b'='))
        {
            return Err("cache_stage_claim_invalid");
        }
        body.push_str(CLAIM_OUTPUT_KEY);
        body.push('=');
        body.push_str(&claim);
        body.push('\n');
    }
    if let Some(compression) = outcome.compression {
        body.push_str(COMPRESSION_OUTPUT_KEY);
        body.push('=');
        body.push_str(compression.as_str());
        body.push('\n');
    }
    Ok(Some(body))
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

#[cfg(test)]
mod tests {
    use super::outcome_body;
    use velnor_actions_orchestrator::cache_action_stage::{CacheActionCompression, TarOutcome};

    #[test]
    fn cache_action_compression_outputs_preserve_all_sdk_version_methods() {
        let gzip = TarOutcome {
            receipt: None,
            compression: Some(CacheActionCompression::Gzip),
        };
        let zstd_without_long = TarOutcome {
            receipt: None,
            compression: Some(CacheActionCompression::ZstdWithoutLong),
        };
        let zstd = TarOutcome {
            receipt: None,
            compression: Some(CacheActionCompression::Zstd),
        };
        assert_eq!(
            outcome_body(&gzip),
            Ok(Some("cache-stage-compression=gzip\n".to_owned()))
        );
        assert_eq!(
            outcome_body(&zstd_without_long),
            Ok(Some(
                "cache-stage-compression=zstd-without-long\n".to_owned()
            ))
        );
        assert_eq!(
            outcome_body(&zstd),
            Ok(Some("cache-stage-compression=zstd\n".to_owned()))
        );
    }
}

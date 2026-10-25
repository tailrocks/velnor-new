use velnor_actions_contract::validate_digest;
use velnor_actions_mise::CachedTaskDescriptor;
use velnor_actions_mise::cache::verify_artifact_digest;
use velnor_actions_mise::restore::MissReason;
use velnor_actions_mise::restore_evidence::output_bytes_complete;

#[derive(Debug, Clone)]
pub(crate) struct ExpectedReuseIdentity {
    pub(crate) cache_key: String,
    pub(crate) compatibility_id: String,
    pub(crate) owner_scope: String,
    pub(crate) input_digest: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ObservedRestoreMeta {
    pub(crate) task_id: String,
    pub(crate) key: String,
    pub(crate) compat: String,
    pub(crate) owner: String,
}

pub(crate) type ObservedOutput = (String, Vec<u8>, String);

fn check_eligibility(
    declared: &[String],
    descriptor: Option<&CachedTaskDescriptor>,
) -> Result<(), MissReason> {
    if declared.is_empty() {
        return Err(MissReason::TASK_NOT_ELIGIBLE);
    }
    let Some(descriptor) = descriptor else {
        return Ok(());
    };
    if descriptor.sources.is_empty() || descriptor.outputs.is_empty() || descriptor.tools.is_empty()
    {
        return Err(MissReason::TASK_NOT_ELIGIBLE);
    }
    descriptor
        .validate()
        .map_err(|_| MissReason::TASK_NOT_ELIGIBLE)
}

fn check_presence(observed: &[ObservedOutput]) -> Result<(), MissReason> {
    if observed.is_empty() {
        return Err(MissReason::TASK_RESULT_INCOMPLETE);
    }
    Ok(())
}

fn check_restored(declared: &[String], observed: &[ObservedOutput]) -> Result<(), MissReason> {
    for (_, bytes, digest) in observed {
        if !output_bytes_complete(bytes) {
            return Err(MissReason::TASK_RESULT_INCOMPLETE);
        }
        if validate_digest(digest).is_err() {
            return Err(MissReason::CACHE_CORRUPT);
        }
        if verify_artifact_digest(bytes, digest).is_err() {
            return Err(MissReason::CACHE_CORRUPT);
        }
    }
    for path in declared {
        if !observed.iter().any(|(name, _, _)| name == path) {
            return Err(MissReason::TASK_RESULT_INCOMPLETE);
        }
    }
    Ok(())
}

fn check_verified(
    expected: Option<&ExpectedReuseIdentity>,
    restore: Option<&ObservedRestoreMeta>,
    task_id: &str,
    live_input_digest: Option<&str>,
) -> Result<(), MissReason> {
    let (Some(expected), Some(restore)) = (expected, restore) else {
        return Err(MissReason::TASK_RESULT_INCOMPLETE);
    };
    if task_id.is_empty() || restore.task_id != task_id {
        return Err(MissReason::NO_ENTRY);
    }
    if validate_digest(&expected.cache_key).is_err()
        || validate_digest(&expected.compatibility_id).is_err()
        || validate_digest(&expected.input_digest).is_err()
        || expected.owner_scope.is_empty()
    {
        return Err(MissReason::INPUT_DIGEST_MISMATCH);
    }
    if validate_digest(&restore.key).is_err()
        || validate_digest(&restore.compat).is_err()
        || restore.owner.is_empty()
    {
        return Err(MissReason::CACHE_CORRUPT);
    }
    if restore.key != expected.cache_key {
        return Err(MissReason::INPUT_DIGEST_MISMATCH);
    }
    if restore.compat != expected.compatibility_id {
        return Err(MissReason::COMPATIBILITY_MISMATCH);
    }
    if restore.owner != expected.owner_scope {
        return Err(MissReason::TRUST_SCOPE_MISMATCH);
    }
    let Some(live) = live_input_digest else {
        return Err(MissReason::TASK_RESULT_INCOMPLETE);
    };
    if validate_digest(live).is_err() || live != expected.input_digest {
        return Err(MissReason::INPUT_DIGEST_MISMATCH);
    }
    Ok(())
}

pub(crate) fn verify_reused_pipeline(
    declared: &[String],
    observed: &[ObservedOutput],
    descriptor: Option<&CachedTaskDescriptor>,
    expected: Option<&ExpectedReuseIdentity>,
    restore: Option<&ObservedRestoreMeta>,
    task_id: &str,
    live_input_digest: Option<&str>,
) -> Result<(), MissReason> {
    check_presence(observed)?;
    check_eligibility(declared, descriptor)?;
    check_restored(declared, observed)?;
    check_verified(expected, restore, task_id, live_input_digest)
}

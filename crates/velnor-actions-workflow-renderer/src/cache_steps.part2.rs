fn mbx_action_identity(step: &Step, id: &str) -> Result<(String, String), RenderError> {
    let StepKind::Action { with, .. } = &step.kind else {
        return Err(RenderError::InvalidWorkflow(format!("mbx_action_invalid:{id}")));
    };
    if with.get("github-cache-mode").map(String::as_str) != Some("objects")
        || with.contains_key("version")
    {
        return Err(RenderError::InvalidWorkflow(format!("mbx_action_inputs_invalid:{id}")));
    }
    let generation = with.get("cache-generation").ok_or_else(|| {
        RenderError::InvalidWorkflow(format!("mbx_cache_generation_missing:{id}"))
    })?;
    let version = generation
        .strip_prefix(MBX_CACHE_GENERATION_PREFIX)
        .filter(|version| is_exact_mbx_version(version))
        .ok_or_else(|| RenderError::InvalidWorkflow(format!("mbx_cache_generation_invalid:{id}")))?
        .to_owned();
    if *generation != mbx_cache_generation(&version) {
        return Err(RenderError::InvalidWorkflow(format!(
            "mbx_cache_generation_override:{id}"
        )));
    }
    let toolchain = with.get("toolchain").cloned().ok_or_else(|| {
        RenderError::InvalidWorkflow(format!("mbx_toolchain_missing:{id}"))
    })?;
    Ok((version, toolchain))
}

/// Whether a job maps to a supported hosted target rather than a selector.
pub(crate) fn has_hosted_cache_target(job: &Job, default_label: &str) -> bool {
    hosted_cache_target(job, default_label).is_some()
}

fn hosted_cache_target(job: &Job, default_label: &str) -> Option<&'static str> {
    let is_scale_set = RunsOn::parse(&job.runs_on).is_ok_and(|selector| selector.is_scale_set());
    (!is_scale_set)
        .then(|| crate::job_runners::cache_target_for_job(job, default_label))
        .flatten()
}

/// True when shell argv invokes the `mbx` program or tool spec.
fn uses_mbx_tool(step: &Step) -> bool {
    matches!(&step.kind, velnor_actions_contract::StepKind::Shell { run, .. } if run.iter().any(|arg| arg == "mbx" || arg.contains("mr-boxington")))
}

/// Exact MBX versions: three nonempty numeric dot parts, nothing else.
fn is_exact_mbx_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

/// Cache restore/save step over `actions/cache`; MBX never archives here.
/// # Errors
pub fn cache_action_step(
    restore: bool,
    uses: &str,
    layer: &str,
    key: &str,
    restore_keys: &[String],
    paths: &[String],
) -> Result<Step, RenderError> {
    validate_uses(uses)?;
    let want = if restore {
        CACHE_RESTORE_NAME
    } else {
        CACHE_SAVE_NAME
    };
    if !uses.starts_with(&format!("{want}@")) {
        return Err(RenderError::BadActionRef(format!("bad_cache_uses:{uses}")));
    }
    if !matches!(layer, "sources" | "task" | "tools" | "tofu-providers" | "mbx") {
        return Err(RenderError::BadCommand("mbx_needs_objects_mode".to_owned()));
    }
    if !cache_key_shape_ok(key) || key.contains('\n') || key.contains('\r') {
        return Err(RenderError::BadCommand("bad_cache_key".to_owned()));
    }
    crate::expressions::check_with_value("key", key)?;
    for restore_key in restore_keys {
        if !cache_key_shape_ok(restore_key)
            || restore_key.contains('\n')
            || restore_key.contains('\r')
        {
            return Err(RenderError::BadCommand("bad_cache_restore_key".to_owned()));
        }
        crate::expressions::check_with_value("restore-keys", restore_key)?;
    }
    if paths.is_empty() {
        return Err(RenderError::BadCommand("empty_cache_paths".to_owned()));
    }
    for path in paths {
        validate_cache_path(layer, path)?;
    }
    let mut with = BTreeMap::from([
        ("key".to_owned(), key.to_owned()),
        ("path".to_owned(), paths.join("\n")),
    ]);
    if restore {
        with.insert("restore-keys".to_owned(), restore_keys.join("\n"));
    }
    let name = if restore {
        "Restore cache"
    } else {
        "Save cache"
    };
    action_step(name, uses, with)
}

fn validate_cache_path(layer: &str, path: &str) -> Result<(), RenderError> {
    if layer == "sources" && sources_subset_ok(path)
        || layer == "task" && path == TASK_ARTIFACTS_DIR
        || layer == "tools" && tools::tools_cache_path_ok(path)
        || layer == "tofu-providers" && crate::tofu_cache::tofu_providers_path_ok(path)
        || layer == "mbx" && path == crate::stock_mbx_bundle::MBX_BUNDLE_CACHE_PATH
    {
        Ok(())
    } else {
        Err(RenderError::BadCommand(format!("bad_cache_path:{path}")))
    }
}

fn cache_key_shape_ok(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    let mut rest = value;
    while let Some(start) = rest.find("${{") {
        if !rest[..start].bytes().all(is_cache_key_literal_byte) {
            return false;
        }
        let Some(end) = rest[start + 3..].find("}}") else {
            return false;
        };
        rest = &rest[start + 3 + end + 2..];
    }
    rest.bytes().all(is_cache_key_literal_byte)
}

fn is_cache_key_literal_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
}

/// P08 sources subset under the owned Cargo home expression.
fn sources_subset_ok(path: &str) -> bool {
    const HOME: &str = "${{ runner.temp }}/velnor/cargo";
    if path.contains("..") || is_never_archive_path(path) {
        return false;
    }
    let Some(suffix) = path.strip_prefix(&format!("{HOME}/")) else {
        return false;
    };
    if suffix.starts_with("registry/src") {
        return false;
    }
    matches!(suffix, "registry/index" | "registry/cache" | "git/db")
        || suffix.starts_with("registry/index/")
        || suffix.starts_with("registry/cache/")
        || suffix.starts_with("git/db/")
}

/// Check restore-before/save-after ordering over cache action steps.
///
/// Every cache download and MBX objects restore must precede every save.
/// A lookup-only restore is an exact-key existence check; it may follow a
/// save because it does not download or mutate the archived path.
/// # Errors
pub fn check_cache_step_order(steps: &[Step]) -> Result<(), RenderError> {
    let mut last_restore: Option<usize> = None;
    let mut first_save: Option<usize> = None;
    for (index, step) in steps.iter().enumerate() {
        let StepKind::Action { uses, .. } = &step.kind else {
            continue;
        };
        if uses == TOOLS_RESTORE_USES
            || uses.starts_with("actions/cache/restore@")
            || uses.starts_with(&format!("{MBX_ACTION_NAME}@"))
        {
            last_restore = Some(index);
        } else if uses.starts_with("actions/cache/save@") {
            first_save.get_or_insert(index);
        }
    }
    if let (Some(restore), Some(save)) = (last_restore, first_save)
        && restore > save
    {
        return Err(RenderError::InvalidWorkflow(
            "cache_save_before_restore".to_owned(),
        ));
    }
    Ok(())
}

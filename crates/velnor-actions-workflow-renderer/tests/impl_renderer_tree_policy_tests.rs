use super::*;

#[test]
fn rendered_yaml_contains_no_private_subcommands() -> Result<(), Box<dyn Error>> {
    let ir = fixture_ir()?;
    let mut ctx = fixture_ctx();
    ctx.validator_commands = validator_commands();
    let support =
        WorkflowPolicy::VelnorRepositoryV1.support_workflow(GeneratorValidation::Bootstrap);
    let text = render_workflow_ir(
        &ir,
        WorkflowPolicy::VelnorRepositoryV1,
        Some(&support),
        &ctx,
    )?;
    for token in FORBIDDEN_TOKENS {
        assert!(!text.contains(token), "leaked token: {token}");
    }
    assert!(text.contains(&format!("{INTERNAL_OP_ENV}: plan-v1")));
    assert!(text.contains(&format!(
        "{REQUEST_FILE_ENV}: ${{{{ runner.temp }}}}/velnor/r1-a1/plan-v1-request.json"
    )));
    let rendered_run = rendered_plan_run(&text)
        .ok_or_else(|| std::io::Error::other("rendered plan step has no single-line run scalar"))?;
    let root = scratch_path("rendered-plan-argv")?;
    // A matching path makes an accidental unquoted glob expand during argv execution.
    let glob_match = root.join("Runner Space/literal-match/velnor/bin");
    fs::create_dir_all(&glob_match)?;
    fs::write(
        glob_match.join(format!("velnor-actions-{VERSION}")),
        b"staged",
    )?;
    let runner_temp = root
        .join("Runner Space/[l]iteral*")
        .to_string_lossy()
        .into_owned();
    let expected = ctx.staged_binary.replace("$RUNNER_TEMP", &runner_temp);
    assert_eq!(
        crate::impl_renderer_steps_quote::shell_argv(
            &rendered_run,
            Some(&runner_temp),
            "/tmp/Home Space",
        )?,
        vec![expected]
    );
    fs::remove_dir_all(root)?;
    Ok(())
}

fn rendered_plan_run(yaml: &str) -> Option<String> {
    // Decode the emitted scalar, then test shell argv semantics instead of YAML bytes.
    let mut in_plan_step = false;
    let mut run = None;
    for line in yaml.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("- name: ") {
            in_plan_step = trimmed == "- name: Plan";
            continue;
        }
        if in_plan_step && let Some(scalar) = trimmed.strip_prefix("run: ") {
            if run.is_some() {
                return None;
            }
            run = decode_yaml_scalar(scalar);
        }
    }
    run
}

fn decode_yaml_scalar(scalar: &str) -> Option<String> {
    if scalar.starts_with('\'') {
        let body = scalar.strip_prefix('\'')?.strip_suffix('\'')?;
        let mut decoded = String::with_capacity(body.len());
        let mut chars = body.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\'' {
                chars.next_if_eq(&'\'')?;
            }
            decoded.push(ch);
        }
        return Some(decoded);
    }
    if scalar.starts_with('"') {
        let body = scalar.strip_prefix('"')?.strip_suffix('"')?;
        let mut decoded = String::with_capacity(body.len());
        let mut chars = body.chars();
        while let Some(ch) = chars.next() {
            if ch == '\\' {
                decoded.push(match chars.next()? {
                    '"' => '"',
                    '\\' => '\\',
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    _ => return None,
                });
            } else {
                decoded.push(ch);
            }
        }
        return Some(decoded);
    }
    Some(scalar.to_owned())
}

fn scratch_path(name: &str) -> Result<PathBuf, std::io::Error> {
    let temp_root = fs::canonicalize(std::env::temp_dir())?;
    let root = temp_root.join(format!("velnor-{name}-{}", std::process::id()));
    if root.exists() {
        fs::remove_dir_all(&root)?;
    }
    Ok(root)
}

#[test]
fn internal_gate_requires_staged_binary_and_request_dir() -> Result<(), RenderError> {
    let ir = fixture_ir()?;
    let mut ctx = fixture_ctx();
    ctx.staged_binary = "/tmp/velnor-actions-0.1.0".to_owned();
    assert!(render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx).is_err());
    let mut ctx = fixture_ctx();
    ctx.staged_binary = "$RUNNER_TEMP/velnor/bin/velnor-actions-9.9.9".to_owned();
    assert!(render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx).is_err());
    let mut ctx = fixture_ctx();
    ctx.request_dir = "/tmp/velnor".to_owned();
    assert!(render_workflow_ir(&ir, WorkflowPolicy::ConsumerV1, None, &ctx).is_err());
    Ok(())
}

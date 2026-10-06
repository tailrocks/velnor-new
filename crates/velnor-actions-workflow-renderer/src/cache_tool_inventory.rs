//! Tool footprint derives from visible selectors and qualified owner records.
use super::is_tool_spec;
use std::collections::BTreeSet;
use velnor_actions_contract::{Job, StepKind};

/// Union of `mise install`/`exec` specs across a job's shell steps.
#[must_use]
pub fn infer_job_tools(job: &Job) -> Vec<String> {
    let mut specs: BTreeSet<_> = infer_job_selectors(job).into_iter().collect();
    for step in &job.steps {
        if matches!(&step.kind, StepKind::SourceBoundHelper { invocation, .. }
            if invocation.descriptor().operation() == velnor_actions_contract::SourceBoundOperation::MiseBootstrap)
        {
            continue;
        }
        let (StepKind::Shell { env, .. } | StepKind::SourceBoundHelper { env, .. }) = &step.kind
        else {
            continue;
        };
        if let Some(identity) = env.get("VELNOR_TOOL_CACHE_IDENTITY")
            && is_tool_spec(identity)
        {
            specs.insert(identity.clone());
        }
        if let Some(identity) = env.get("VELNOR_QUALIFIED_TOOL_IDENTITY")
            && is_tool_spec(identity)
        {
            specs.insert(identity.clone());
        }
        if let Some(identity) = env.get("VELNOR_RUSTUP_IDENTITY")
            && is_tool_spec(identity)
        {
            specs.insert(identity.clone());
        }
    }
    specs.into_iter().collect()
}

/// Actual installation footprint, excluding synthetic cache identity inputs.
#[must_use]
pub fn infer_job_selectors(job: &Job) -> Vec<String> {
    let mut selectors = BTreeSet::new();
    for step in &job.steps {
        match &step.kind {
            StepKind::Shell { run, .. } => selectors.extend(specs_in_argv(run)),
            StepKind::SourceBoundHelper { invocation, .. } => {
                selectors.extend(invocation.installed_selectors().iter().cloned());
            }
            _ => {}
        }
    }
    selectors.into_iter().collect()
}

/// Tool specs (`<tool>@<version>`) in one fixed argv.
fn specs_in_argv(run: &[String]) -> Vec<String> {
    if crate::commands::inline_script_index(run).is_some() {
        return Vec::new();
    }
    let mise = crate::toolchain_env::unset_prefix_len(run);
    if run.get(mise).is_none_or(|argument| argument != "mise") {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut take = false;
    for arg in &run[mise + 1..] {
        if arg == "install" || arg == "exec" {
            take = true;
            continue;
        }
        if arg == "--" {
            break;
        }
        if take && arg.contains('@') && is_tool_spec(arg) {
            out.push(arg.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn job(argv: &[&str]) -> Job {
        Job {
            cache_mode: None,
            display_name: "Inventory".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            timeout_minutes: velnor_actions_contract::JobTimeout::VALIDATOR,
            needs: Vec::new(),
            condition: None,
            permissions: None,
            tool_producer: None,
            mbx_producer: None,
            source_producer: None,
            native_pages_deploy: None,
            native_publish: None,
            outputs: Vec::new(),
            environment: None,
            steps: vec![
                crate::steps::ambient_shell_step(
                    "Pinned argv",
                    argv.iter().map(|word| (*word).to_owned()).collect(),
                    std::collections::BTreeMap::new(),
                )
                .expect("argv"),
            ],
        }
    }
    #[test]
    fn literal_pinned_mise_selectors_are_visible() {
        let job = job(&["mise", "--no-config", "exec", "gh@2.102.0", "--", "gh"]);
        assert_eq!(infer_job_selectors(&job), vec!["gh@2.102.0"]);
    }
    #[test]
    fn opaque_shell_and_argument_text_never_grant_selector_authority() {
        for argv in [
            vec!["sh", "-c", "mise install gh@2.102.0"],
            vec!["printf", "mise", "install", "gh@2.102.0"],
        ] {
            assert!(infer_job_selectors(&job(&argv)).is_empty());
        }
    }
    #[test]
    fn exec_program_arguments_cannot_expand_the_footprint() {
        let job = job(&[
            "mise",
            "exec",
            "gh@2.102.0",
            "--",
            "gh",
            "install",
            "rust@1.98.1",
        ]);
        assert_eq!(infer_job_selectors(&job), vec!["gh@2.102.0"]);
    }
}

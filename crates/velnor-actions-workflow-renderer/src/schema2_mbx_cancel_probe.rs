//! Hosted MBX qualification for actual workflow cancellation.
//!
//! Each case dispatches one exact protected-main victim run, cancels only its
//! returned ID, and observes its run-scoped cache key from a fresh job.

use std::collections::BTreeMap;

use super::MbxQualificationPins;
use crate::yaml::Yaml;
use crate::{RenderError, mbx_bundle};
use velnor_actions_contract::{Job, PermissionLevel, PullRequestCachePolicy, Step};

#[path = "schema2_mbx_cancel_probe_policy.rs"]
mod policy;
#[path = "schema2_mbx_cancel_probe_private_io.rs"]
mod private_io;
#[path = "schema2_mbx_cancel_probe_steps.rs"]
mod probe_steps;
#[path = "schema2_mbx_cancel_probe_render.rs"]
mod render;
#[path = "schema2_mbx_cancel_probe_scripts.rs"]
mod scripts;

#[cfg(test)]
#[path = "schema2_mbx_cancel_probe_tests.rs"]
mod tests;

const PRE_SCOPE: &str = "qualification-mbx-v1/cancel-pre-save-victim";
const SAVE_SCOPE: &str = "qualification-mbx-v1/cancel-during-save-victim";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    PreSave,
    DuringSave,
}

impl Phase {
    fn token(self) -> &'static str {
        match self {
            Self::PreSave => "pre-save",
            Self::DuringSave => "during-save",
        }
    }

    fn controller_mode(self) -> &'static str {
        match self {
            Self::PreSave => "mbx-cancel-pre-save-controller",
            Self::DuringSave => "mbx-cancel-during-save-controller",
        }
    }

    fn victim_mode(self) -> &'static str {
        match self {
            Self::PreSave => "mbx-cancel-pre-save-victim",
            Self::DuringSave => "mbx-cancel-during-save-victim",
        }
    }

    fn scope(self) -> &'static str {
        match self {
            Self::PreSave => PRE_SCOPE,
            Self::DuringSave => SAVE_SCOPE,
        }
    }

    fn controller_id(self) -> &'static str {
        match self {
            Self::PreSave => "mbx-cancel-pre-save-controller",
            Self::DuringSave => "mbx-cancel-during-save-controller",
        }
    }

    fn victim_id(self) -> &'static str {
        match self {
            Self::PreSave => "mbx-cancel-pre-save-victim",
            Self::DuringSave => "mbx-cancel-during-save-victim",
        }
    }

    fn observer_id(self) -> &'static str {
        match self {
            Self::PreSave => "mbx-cancel-pre-save-observer",
            Self::DuringSave => "mbx-cancel-during-save-observer",
        }
    }

    fn mode(self, role: &str) -> &'static str {
        match role {
            "victim" => self.victim_mode(),
            _ => self.controller_mode(),
        }
    }

    fn gate(self, role: &str) -> String {
        format!(
            "inputs.mode == '{}' && github.event_name == 'workflow_dispatch' && github.ref == 'refs/heads/main' && github.ref_protected == true",
            self.mode(role)
        )
    }
}

/// Build the hosted controller, victim, and fresh observer for both cases.
///
/// # Errors
/// Invalid action, Mise, MBX, Rust, or runner pins fail closed.
pub(super) fn jobs(
    request: &MbxQualificationPins,
    hosted: &Yaml,
) -> Result<Vec<(String, Yaml)>, RenderError> {
    probe_steps::validate_request(request, hosted)?;
    let mut victims = BTreeMap::from([
        (
            Phase::PreSave.victim_id().to_owned(),
            victim_job(request, Phase::PreSave, hosted)?,
        ),
        (
            Phase::DuringSave.victim_id().to_owned(),
            victim_job(request, Phase::DuringSave, hosted)?,
        ),
    ]);
    mbx_bundle::append_single_bundle_saves(&mut victims, PullRequestCachePolicy::ReadOnly)?;
    for (id, job) in &mut victims {
        policy::pin_unmatrixed_key_context(job)?;
        policy::gate_victim_writer_steps(job, id == Phase::DuringSave.victim_id());
    }

    let mut output = Vec::with_capacity(6);
    for phase in [Phase::PreSave, Phase::DuringSave] {
        let victim = victims.remove(phase.victim_id()).ok_or_else(|| {
            RenderError::InvalidWorkflow(format!("mbx_cancel_missing_victim:{}", phase.token()))
        })?;
        output.push(render::render_victim_job(
            victim,
            phase.victim_id(),
            hosted,
            phase,
            request,
        )?);
        output.push(controller_job(request, phase, hosted)?);
        output.push(observer_job(request, phase, hosted)?);
    }
    Ok(output)
}

fn victim_job(
    request: &MbxQualificationPins,
    phase: Phase,
    hosted: &Yaml,
) -> Result<Job, RenderError> {
    let steps = vec![
        probe_steps::mise_setup_step(request)?,
        probe_steps::mise_install_step(request)?,
        probe_steps::mbx_action_step(request, phase, true)?,
        probe_steps::verify_action_step(request)?,
    ];
    let mut steps = steps;
    steps.push(probe_steps::upload_victim_receipt_step(phase)?);
    let job = make_job(
        format!("MBX cancellation / {} victim", phase.token()),
        hosted,
        phase.gate("victim"),
        Vec::new(),
        PermissionLevel::Write,
        steps,
        45,
    )?;
    Ok(job)
}

fn controller_job(
    request: &MbxQualificationPins,
    phase: Phase,
    hosted: &Yaml,
) -> Result<(String, Yaml), RenderError> {
    let id = phase.controller_id();
    let Yaml::Str(runs_on) = hosted else {
        return Err(RenderError::InvalidWorkflow(
            "mbx_cancel_requires_hosted_runner".to_owned(),
        ));
    };
    let steps = vec![
        render::bash_step(
            "Generate fresh cancellation probe ID",
            Some("cancel-probe-id"),
            scripts::GENERATE_ID,
            &BTreeMap::new(),
        ),
        probe_steps::api_shell_step(probe_steps::ApiShellSpec {
            name: "Dispatch exact protected-main victim",
            script: scripts::DISPATCH,
            request,
            phase,
            probe_id: "${{ steps.cancel-probe-id.outputs.probe_id }}",
            child_run_id: None,
            child_workflow_id: None,
            id: Some("cancel-dispatch"),
            condition: None,
        })?,
        probe_steps::api_shell_step(probe_steps::ApiShellSpec {
            name: "Wait for exact victim readiness",
            script: &scripts::wait_readiness(),
            request,
            phase,
            probe_id: "${{ steps.cancel-probe-id.outputs.probe_id }}",
            child_run_id: Some("${{ steps.cancel-dispatch.outputs.workflow_run_id }}"),
            child_workflow_id: Some("${{ steps.cancel-dispatch.outputs.workflow_id }}"),
            id: Some("cancel-ready"),
            condition: None,
        })?,
        probe_steps::api_shell_step(probe_steps::ApiShellSpec {
            name: "Cancel exact in-progress victim",
            script: &scripts::cancel_exact(),
            request,
            phase,
            probe_id: "${{ steps.cancel-probe-id.outputs.probe_id }}",
            child_run_id: Some("${{ steps.cancel-dispatch.outputs.workflow_run_id }}"),
            child_workflow_id: Some("${{ steps.cancel-dispatch.outputs.workflow_id }}"),
            id: Some("cancel-request"),
            condition: None,
        })?,
        probe_steps::api_shell_step(probe_steps::ApiShellSpec {
            name: "Wait for exact victim terminal state",
            script: &scripts::wait_terminal(),
            request,
            phase,
            probe_id: "${{ steps.cancel-probe-id.outputs.probe_id }}",
            child_run_id: Some("${{ steps.cancel-dispatch.outputs.workflow_run_id }}"),
            child_workflow_id: Some("${{ steps.cancel-dispatch.outputs.workflow_id }}"),
            id: Some("cancel-terminal"),
            condition: None,
        })?,
        probe_steps::controller_receipt_yaml(request, phase),
        render::typed_step(
            id,
            &probe_steps::upload_controller_receipt_step(phase)?,
            runs_on,
        )?,
    ];
    let job = make_job(
        format!("MBX cancellation / {} controller", phase.token()),
        hosted,
        phase.gate("controller"),
        Vec::new(),
        PermissionLevel::Write,
        Vec::new(),
        60,
    )?;
    render::render_raw_job(job, id, hosted, steps)
}

fn observer_job(
    request: &MbxQualificationPins,
    phase: Phase,
    hosted: &Yaml,
) -> Result<(String, Yaml), RenderError> {
    let id = phase.observer_id();
    let Yaml::Str(runs_on) = hosted else {
        return Err(RenderError::InvalidWorkflow(
            "mbx_cancel_requires_hosted_runner".to_owned(),
        ));
    };
    let mut steps = observer_start_steps(request, phase, id, runs_on)?;
    steps.extend(observer_reader_steps(request, phase, id, runs_on)?);
    steps.extend(observer_work_steps(request));
    steps.push(probe_steps::observer_evidence_step(request, phase)?);
    steps.push(probe_steps::observer_classify_step(request, phase));
    steps.push(render::typed_step(
        id,
        &probe_steps::upload_observer_receipt_step(phase)?,
        runs_on,
    )?);
    let gate = format!("always() && ({})", phase.gate("observer"));
    let job = make_job(
        format!("MBX cancellation / {} fresh observer", phase.token()),
        hosted,
        gate,
        vec![phase.controller_id().to_owned()],
        PermissionLevel::Read,
        Vec::new(),
        60,
    )?;
    render::render_raw_job(job, id, hosted, steps)
}

fn observer_start_steps(
    request: &MbxQualificationPins,
    phase: Phase,
    id: &str,
    runs_on: &str,
) -> Result<Vec<Yaml>, RenderError> {
    let mut steps = vec![
        probe_steps::observer_init_step(),
        render::typed_step(
            id,
            &probe_steps::download_controller_receipt_step(phase)?,
            runs_on,
        )?,
        probe_steps::api_shell_step(probe_steps::ApiShellSpec {
            name: "Validate controller receipt and child run",
            script: scripts::VALIDATE_CONTROLLER_RECEIPT,
            request,
            phase,
            probe_id: "",
            child_run_id: None,
            child_workflow_id: None,
            id: Some("mbx-cancel-receipt"),
            condition: None,
        })?,
    ];
    for mut step in [
        probe_steps::mise_setup_step(request)?,
        probe_steps::mise_install_step(request)?,
        probe_steps::mbx_action_step(request, phase, false)?,
        probe_steps::verify_action_step(request)?,
    ] {
        gate_observer_step(&mut step);
        steps.push(render::typed_step(id, &step, runs_on)?);
    }
    Ok(steps)
}

fn observer_reader_steps(
    request: &MbxQualificationPins,
    phase: Phase,
    id: &str,
    runs_on: &str,
) -> Result<Vec<Yaml>, RenderError> {
    let binding = mbx_bundle::QualificationObserverBinding {
        child_run_id: "${{ steps.mbx-cancel-receipt.outputs.child_run_id }}",
        child_attempt: "${{ steps.mbx-cancel-receipt.outputs.child_attempt }}",
        source_sha: "${{ steps.mbx-cancel-receipt.outputs.source_sha }}",
        receipt_primary: "${{ steps.mbx-cancel-receipt.outputs.cache_key }}",
        receipt_generation: "${{ steps.mbx-cancel-receipt.outputs.generation }}",
        receipt_rustc_identity: "${{ steps.mbx-cancel-receipt.outputs.rustc_identity }}",
        receipt_version: "${{ steps.mbx-cancel-receipt.outputs.mbx_version }}",
    };
    let reader_steps = mbx_bundle::qualification_observer_steps(
        phase.scope(),
        &request.mbx_version,
        &probe_steps::rust_env(request),
        &binding,
    )?;
    let mut reader_steps = reader_steps.into_iter();
    let mut key_step = reader_steps.next().ok_or_else(|| {
        RenderError::InvalidWorkflow("mbx_cancel_observer_key_step_missing".to_owned())
    })?;
    if mbx_bundle::step_yaml_id(&key_step) != Some("mbx-bundle-key") {
        return Err(RenderError::InvalidWorkflow(
            "mbx_cancel_observer_key_step_misordered".to_owned(),
        ));
    }
    policy::pin_observer_key_step(&mut key_step)?;
    gate_observer_step(&mut key_step);
    let mut steps = vec![render::typed_step(id, &key_step, runs_on)?];
    steps.push(probe_steps::observer_cache_before_step(request, phase)?);
    for mut step in reader_steps {
        gate_observer_step(&mut step);
        steps.push(render::typed_step(id, &step, runs_on)?);
    }
    Ok(steps)
}

fn observer_work_steps(request: &MbxQualificationPins) -> Vec<Yaml> {
    vec![
        probe_steps::source_fetch_yaml(Some(
            "steps.mbx-cancel-receipt.outputs.should_observe == 'true'",
        )),
        probe_steps::observer_measure_import_step(),
        probe_steps::workspace_build_yaml(
            request,
            Some("steps.mbx-cancel-receipt.outputs.should_observe == 'true'"),
        ),
        probe_steps::observer_measure_reuse_step(),
    ]
}

fn make_job(
    title: String,
    hosted: &Yaml,
    gate: String,
    needs: Vec<String>,
    actions: PermissionLevel,
    steps: Vec<Step>,
    timeout_minutes: u16,
) -> Result<Job, RenderError> {
    probe_steps::make_job(title, hosted, gate, needs, actions, steps, timeout_minutes)
}

fn gate_observer_step(step: &mut Step) {
    let observe = "steps.mbx-cancel-receipt.outputs.should_observe == 'true'";
    step.condition = Some(match step.condition.take() {
        Some(condition) => format!("({condition}) && {observe}"),
        None => observe.to_owned(),
    });
}

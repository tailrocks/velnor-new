//! Scheduled upstream-freshness workflow rendering (P12-4 emission half).
//!
//! Third render entrypoint: [`render_freshness_workflow`] turns a
//! validated [`FreshnessSpec`] into `freshness.yml` — a schedule-only,
//! read-only probe that runs the bounded
//! `scripts/check-freshness.sh --check-upstream` over the Velnor-owned
//! inventory. The contract half ([`ScheduleTrigger`],
//! [`FRESHNESS_CRON_WEEKLY`], [`FRESHNESS_WORKFLOW_PATH`]) lives in
//! `velnor-actions-contract`; the main CI renderer cannot serve here
//! because it mandates PR/push/merge-group triggers plus plan, task,
//! and final closures.
//!
//! [`ScheduleTrigger`]: velnor_actions_contract::ScheduleTrigger
//! [`FRESHNESS_CRON_WEEKLY`]: velnor_actions_contract::FRESHNESS_CRON_WEEKLY
//! [`FRESHNESS_WORKFLOW_PATH`]: velnor_actions_contract::FRESHNESS_WORKFLOW_PATH

use std::collections::BTreeMap;

use velnor_actions_contract::{FRESHNESS_WORKFLOW_PATH, ScheduleTrigger, Step, StepRole};

use crate::{
    RenderError, guard, marker,
    render::RenderedFile,
    steps::{self, action_step, shell_step},
    yaml::{Yaml, render_yaml},
};

/// Display name of the freshness workflow.
pub const FRESHNESS_WORKFLOW_NAME: &str = "Upstream Freshness";
/// Stable freshness job ID.
pub const FRESHNESS_JOB_ID: &str = "freshness";
/// Display name of the freshness job.
pub const FRESHNESS_JOB_NAME: &str = "Freshness";
/// The bounded read-only probe this workflow runs, fixed.
pub const FRESHNESS_SCRIPT: &str = "scripts/check-freshness.sh";
/// Probe flag: refetch upstream evidence, write nothing.
pub const FRESHNESS_PROBE_FLAG: &str = "--check-upstream";
/// Freshness concurrency group (scheduled runs serialize, never cancel).
pub const FRESHNESS_CONCURRENCY_GROUP: &str = "freshness";
/// Freshness job timeout: every row may hit its 10 s probe cap.
pub const FRESHNESS_TIMEOUT_MINUTES: i64 = 10;

/// Validated freshness-workflow inputs.
#[derive(Debug, Clone)]
pub struct FreshnessSpec {
    /// Cron schedule (the reviewed weekly trigger).
    pub schedule: ScheduleTrigger,
    /// Single literal versioned Ubuntu label.
    pub runs_on: String,
    /// Pinned `actions/checkout` ref.
    pub checkout_uses: String,
    /// Exact generator version for the marker.
    pub generator_version: String,
}

impl FreshnessSpec {
    /// Validate every spec scalar before rendering.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError`] describing the first invalid scalar.
    pub fn validate(&self) -> Result<(), RenderError> {
        self.schedule.validate().map_err(RenderError::Contract)?;
        marker::validate_version(&self.generator_version)?;
        guard::validate_runs_on(&self.runs_on)?;
        steps::validate_uses(&self.checkout_uses)?;
        if !self.checkout_uses.starts_with("actions/checkout@") {
            return Err(RenderError::BadActionRef(format!(
                "not_checkout:{}",
                self.checkout_uses
            )));
        }
        Ok(())
    }
}

/// Render the freshness workflow document from a validated spec.
///
/// # Errors
///
/// Returns [`RenderError`] for invalid spec scalars or steps.
pub fn render_freshness_workflow(spec: &FreshnessSpec) -> Result<RenderedFile, RenderError> {
    spec.validate()?;
    let mut checkout = action_step(
        "Checkout",
        &spec.checkout_uses,
        BTreeMap::from([
            ("persist-credentials".to_owned(), "false".to_owned()),
            ("fetch-depth".to_owned(), "1".to_owned()),
        ]),
    )?;
    checkout.role = Some(StepRole::Checkout);
    let probe = shell_step(
        "Check upstream freshness",
        vec![
            "bash".to_owned(),
            FRESHNESS_SCRIPT.to_owned(),
            FRESHNESS_PROBE_FLAG.to_owned(),
        ],
        BTreeMap::new(),
    )?;
    let document = freshness_document(spec, &checkout, &probe)?;
    let document = crate::yaml::quote_run_values_in_yaml(document);
    let text = marker::with_marker(&spec.generator_version, &render_yaml(&document))?;
    crate::workflow_size::check_workflow_size(FRESHNESS_WORKFLOW_PATH, &text)?;
    steps::scan_for_private_subcommands(&text)?;
    Ok(RenderedFile {
        path: FRESHNESS_WORKFLOW_PATH.to_owned(),
        bytes: text,
    })
}

/// Build the freshness document: name, schedule triggers, perms, job.
fn freshness_document(
    spec: &FreshnessSpec,
    checkout: &Step,
    probe: &Step,
) -> Result<Yaml, RenderError> {
    velnor_actions_contract::workflow::step_identity::validate_step_sequence(
        &[checkout.clone(), probe.clone()],
        FRESHNESS_JOB_ID,
    )
    .map_err(RenderError::Contract)?;
    let crons: Vec<Yaml> = spec
        .schedule
        .cron
        .iter()
        .map(|cron| Yaml::Map(vec![("cron".to_owned(), Yaml::str(cron.clone()))]))
        .collect();
    let job = Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(FRESHNESS_JOB_NAME)),
        ("runs-on".to_owned(), Yaml::str(spec.runs_on.clone())),
        (
            "timeout-minutes".to_owned(),
            Yaml::Int(FRESHNESS_TIMEOUT_MINUTES),
        ),
        ("permissions".to_owned(), read_only_permissions()),
        (
            "steps".to_owned(),
            Yaml::Seq(vec![
                crate::steps_plain::plain_step_to_yaml(checkout)?,
                crate::steps_plain::plain_step_to_yaml(probe)?,
            ]),
        ),
    ]);
    Ok(Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(FRESHNESS_WORKFLOW_NAME)),
        (
            "on".to_owned(),
            Yaml::Map(vec![
                ("schedule".to_owned(), Yaml::Seq(crons)),
                ("workflow_dispatch".to_owned(), Yaml::Map(Vec::new())),
            ]),
        ),
        ("permissions".to_owned(), read_only_permissions()),
        (
            "concurrency".to_owned(),
            Yaml::Map(vec![
                ("group".to_owned(), Yaml::str(FRESHNESS_CONCURRENCY_GROUP)),
                ("cancel-in-progress".to_owned(), Yaml::Bool(false)),
            ]),
        ),
        (
            "jobs".to_owned(),
            Yaml::Map(vec![(FRESHNESS_JOB_ID.to_owned(), job)]),
        ),
    ]))
}

/// Read-only top-level/job permissions: the probe only reads.
fn read_only_permissions() -> Yaml {
    Yaml::Map(vec![("contents".to_owned(), Yaml::str("read"))])
}

#[cfg(test)]
mod tests {
    use super::*;
    use velnor_actions_contract::FRESHNESS_CRON_WEEKLY;

    const CHECKOUT: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";

    fn spec() -> FreshnessSpec {
        FreshnessSpec {
            schedule: ScheduleTrigger {
                cron: vec![FRESHNESS_CRON_WEEKLY.to_owned()],
            },
            runs_on: "ubuntu-26.04".to_owned(),
            checkout_uses: CHECKOUT.to_owned(),
            generator_version: "0.1.0".to_owned(),
        }
    }

    #[test]
    fn renders_schedule_only_read_only_probe() {
        let file = render_freshness_workflow(&spec()).expect("render");
        assert_eq!(file.path, ".github/workflows/freshness.yml");
        let yaml = &file.bytes;
        assert!(
            yaml.starts_with("# Generated by Velnor Actions 0.1.0"),
            "marker:\n{yaml}"
        );
        assert!(yaml.contains("cron: 0 6 * * 1"), "weekly trigger:\n{yaml}");
        assert!(
            yaml.contains("workflow_dispatch:"),
            "manual re-run:\n{yaml}"
        );
        assert!(
            !yaml.contains("pull_request") && !yaml.contains("push:"),
            "schedule-only:\n{yaml}"
        );
        assert!(
            yaml.matches("contents: read").count() >= 2,
            "top-level plus job perms:\n{yaml}"
        );
        assert!(!yaml.contains("write"), "read-only:\n{yaml}");
        assert!(!yaml.contains("secrets."), "no secrets:\n{yaml}");
        assert!(
            yaml.contains("uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1"),
            "exact pin:\n{yaml}"
        );
        assert!(
            yaml.contains("persist-credentials: \"false\""),
            "no credentials:\n{yaml}"
        );
        assert!(
            yaml.contains("scripts/check-freshness.sh --check-upstream"),
            "bounded probe:\n{yaml}"
        );
        assert!(
            yaml.contains("timeout-minutes: 10"),
            "explicit timeout:\n{yaml}"
        );
        assert!(
            yaml.contains("group: freshness") && yaml.contains("cancel-in-progress: false"),
            "serialize scheduled runs:\n{yaml}"
        );
    }

    #[test]
    fn rejects_bad_spec_scalars() {
        let bad_cron = FreshnessSpec {
            schedule: ScheduleTrigger {
                cron: vec!["not-a-cron".to_owned()],
            },
            ..spec()
        };
        assert!(render_freshness_workflow(&bad_cron).is_err(), "bad cron");
        let empty_cron = FreshnessSpec {
            schedule: ScheduleTrigger { cron: Vec::new() },
            ..spec()
        };
        assert!(
            render_freshness_workflow(&empty_cron).is_err(),
            "empty cron"
        );
        for runs_on in ["ubuntu-latest", "ubuntu-26.04 ${{ x }}", ""] {
            let bad = FreshnessSpec {
                runs_on: runs_on.to_owned(),
                ..spec()
            };
            assert!(
                render_freshness_workflow(&bad).is_err(),
                "bad label {runs_on}"
            );
        }
        for uses in [
            "actions/checkout@v7.0.1",
            "actions/checkout@short",
            "other/x@3d3c42e5aac5ba805825da76410c181273ba90b1",
        ] {
            let bad = FreshnessSpec {
                checkout_uses: uses.to_owned(),
                ..spec()
            };
            assert!(
                render_freshness_workflow(&bad).is_err(),
                "bad checkout {uses}"
            );
        }
    }

    #[test]
    fn rejects_internal_steps_fail_closed() {
        let internal = Step {
            name: "Plan".to_owned(),
            id: None,
            role: None,
            condition: None,
            kind: velnor_actions_contract::StepKind::Internal {
                operation: "plan-v1".to_owned(),
                env: std::collections::BTreeMap::new(),
            },
        };
        let err = crate::steps_plain::plain_step_to_yaml(&internal).expect_err("rejected");
        assert!(err.to_string().contains("internal_op_rejected"), "{err}");
    }
}

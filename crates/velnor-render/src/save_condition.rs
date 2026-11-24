// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The one place that writes what a save waits for, so the shapes of a job cannot
//! drift apart: "the build finished" and "the audit passed".
//!
//! A build finished when its own step succeeded, or when the test step recorded it
//! (`outputs.built`, a helper that records the build). A skipped step is never a
//! finished build: after an earlier failure every build is skipped, and a save must
//! not follow that. A build the plan left out (its gate does not hold) is the only
//! build that may be missing, and one build must have finished.

use velnor_model::{Gate, PrRun, When};

use crate::gate::{Setup, gate_parts};

/// How a job shows that a build finished.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Built {
    /// A `Build the tests` step with this id.
    Step(String),
    /// The test step with this id records the build in its outputs.
    Recorded(String),
}

impl Built {
    pub(crate) fn finished(&self) -> String {
        match self {
            Self::Step(id) => format!("steps.{id}.outcome == 'success'"),
            Self::Recorded(id) => format!("steps.{id}.outputs.built == 'true'"),
        }
    }
}

/// A build of a job and the gate that decides whether the plan runs it.
#[derive(Clone)]
pub(crate) struct Compiled {
    pub(crate) built: Built,
    pub(crate) gate: Option<Gate>,
    pub(crate) setup: Setup,
}

/// An audit of a job, as the saves and the audit itself need to know it.
#[derive(Clone)]
pub(crate) struct Audited {
    pub(crate) id: String,
    pub(crate) when: When,
    pub(crate) setup: Setup,
}

fn unwrapped(condition: &str) -> &str {
    condition.trim_start_matches("${{ ").trim_end_matches(" }}")
}

/// What a save, or the audit, waits for before the builds `compiled`.
pub(crate) fn builds_finished(compiled: &[Compiled], saved_here: Option<&Gate>) -> Vec<String> {
    let mut required: Vec<String> = Vec::new();
    let mut every_build_is_optional = !compiled.is_empty();
    for build in compiled {
        let finished = build.built.finished();
        let term = match &build.gate {
            Some(gate) if Some(gate) != saved_here => {
                let parts = gate_parts(gate, build.setup == Setup::Replanned).join(" && ");
                format!("({finished} || !({parts}))")
            }
            _ => {
                every_build_is_optional = false;
                finished
            }
        };
        if !required.contains(&term) {
            required.push(term);
        }
    }
    if every_build_is_optional {
        let any: Vec<String> = compiled
            .iter()
            .map(|build| build.built.finished())
            .collect();
        required.push(format!("({})", any.join(" || ")));
    }
    required
}

/// What a save waits for of an audit: it passed, unless its event did not ask for it.
pub(crate) fn audit_passed(audit: &Audited) -> String {
    let passed = format!("steps.{}.outcome == 'success'", audit.id);
    match crate::gate::condition(audit.when, None, audit.setup, PrRun::Always, &[], &[]) {
        Some(asked) => format!("({passed} || !({}))", unwrapped(&asked)),
        None => passed,
    }
}

/// What an audit waits for: no cancellation and the builds before it.
pub(crate) fn audit_runs(compiled: &[Compiled], saved_here: Option<&Gate>) -> Vec<String> {
    std::iter::once("!cancelled()".to_owned())
        .chain(builds_finished(compiled, saved_here))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compiled(built: Built) -> Compiled {
        Compiled {
            built,
            gate: None,
            setup: Setup::Restored,
        }
    }

    #[test]
    fn both_ways_of_finishing_a_build_come_from_the_one_builder() {
        let step = builds_finished(&[compiled(Built::Step("build_1".to_owned()))], None);
        let recorded = builds_finished(&[compiled(Built::Recorded("test".to_owned()))], None);
        assert_eq!(step, ["steps.build_1.outcome == 'success'"]);
        assert_eq!(recorded, ["steps.test.outputs.built == 'true'"]);
    }

    #[test]
    fn the_audit_waits_for_the_same_builds_as_the_saves() {
        for built in [
            Built::Step("build_1".to_owned()),
            Built::Recorded("test".to_owned()),
        ] {
            let builds = [compiled(built.clone())];
            let finished = builds_finished(&builds, None);
            let runs = audit_runs(&builds, None);
            assert_eq!(runs[0], "!cancelled()");
            assert_eq!(runs[1..], finished[..], "{built:?}");
        }
    }

    #[test]
    fn a_pass_is_required_of_an_audit_that_was_asked_for() {
        let audit = Audited {
            id: "audit_1".to_owned(),
            when: When::FullAudit,
            setup: Setup::Restored,
        };
        let passed = audit_passed(&audit);
        assert!(
            passed.starts_with("(steps.audit_1.outcome == 'success' || !("),
            "{passed}"
        );
        assert!(
            passed.contains("github.event_name == 'schedule'"),
            "{passed}"
        );
        assert!(!passed.contains("!= 'failure'"), "{passed}");
    }
}

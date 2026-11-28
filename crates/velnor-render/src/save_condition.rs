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

use std::collections::BTreeMap;

use velnor_model::{Gate, GateNeed, PlanFrom, PrRun, RootSlug, When, mode_env};

use crate::gate::{REPLAN_STEP, Setup, gate_parts, output};

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

fn mode_read(gate: &Gate) -> Option<String> {
    let (PlanFrom::Job(from) | PlanFrom::WorkJob(from)) = &gate.from else {
        return None;
    };
    (gate.need == GateNeed::Selected && gate.also.is_empty()).then(|| {
        let key = output(&gate.slug, GateNeed::Selected);
        format!("${{{{ needs.{from}.outputs.{key} }}}}")
    })
}

/// The plan modes a job reads into its env, named by position over its roots in sorted order.
#[derive(Clone, Debug, Default)]
pub(crate) struct Modes(BTreeMap<RootSlug, String>);

impl Modes {
    pub(crate) fn of<'a>(gates: impl Iterator<Item = &'a Gate>) -> Self {
        Self(
            gates
                .filter_map(|gate| Some((gate.slug.clone(), mode_read(gate)?)))
                .collect(),
        )
    }

    fn name(&self, gate: &Gate) -> Option<String> {
        mode_read(gate)?;
        let index = self.0.keys().position(|slug| *slug == gate.slug)?;
        Some(mode_env(index))
    }

    pub(crate) fn env(&self) -> Vec<(String, String)> {
        self.0
            .values()
            .enumerate()
            .map(|(index, read)| (mode_env(index), read.clone()))
            .collect()
    }
}

/// What a save, or the audit, waits for before the builds `compiled`.
pub(crate) fn builds_finished(
    compiled: &[Compiled],
    saved_here: Option<&Gate>,
    modes: &Modes,
) -> Vec<String> {
    let mut required: Vec<String> = Vec::new();
    let mut every_build_is_optional = !compiled.is_empty();
    for build in compiled {
        let finished = build.built.finished();
        let term = match &build.gate {
            Some(gate) if Some(gate) != saved_here => {
                let parts = match modes.name(gate) {
                    Some(name) => format!("env.{name} != 'none'"),
                    None => gate_parts(gate, false).join(" && "),
                };
                if build.setup == Setup::Replanned {
                    format!(
                        "({finished} || !(steps.{REPLAN_STEP}.outcome == 'success' || ({parts})))"
                    )
                } else {
                    format!("({finished} || !({parts}))")
                }
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
pub(crate) fn audit_runs(
    compiled: &[Compiled],
    saved_here: Option<&Gate>,
    modes: &Modes,
) -> Vec<String> {
    std::iter::once("!cancelled()".to_owned())
        .chain(builds_finished(compiled, saved_here, modes))
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

    fn selected(slug: &'static str) -> Gate {
        Gate {
            slug: RootSlug::from_static(slug),
            need: GateNeed::Selected,
            from: PlanFrom::Job(velnor_model::JobId::from_static("rust-plan")),
            packages: Vec::new(),
            unselected: false,
            group: None,
            also: Vec::new(),
        }
    }

    #[test]
    fn each_root_of_a_job_reads_its_own_mode_named_by_its_position() {
        let gates = [selected("zeta"), selected("alpha")];
        let modes = Modes::of(gates.iter());
        let env = modes.env();
        assert_eq!(env.len(), 2, "{env:?}");
        assert_eq!(env[0].0, "VELNOR_MODE_0");
        assert_eq!(env[1].0, "VELNOR_MODE_1");
        assert!(env[0].1.contains("rust_alpha_mode"), "{env:?}");
        assert!(env[1].1.contains("rust_zeta_mode"), "{env:?}");
        let builds: Vec<Compiled> = gates
            .iter()
            .enumerate()
            .map(|(index, gate)| Compiled {
                built: Built::Step(format!("build_{index}")),
                gate: Some(gate.clone()),
                setup: Setup::Restored,
            })
            .collect();
        let terms = builds_finished(&builds, None, &modes).join(" ");
        assert!(terms.contains("env.VELNOR_MODE_1 != 'none'"), "{terms}");
        assert!(terms.contains("env.VELNOR_MODE_0 != 'none'"), "{terms}");
    }

    #[test]
    fn both_ways_of_finishing_a_build_come_from_the_one_builder() {
        let step = builds_finished(
            &[compiled(Built::Step("build_1".to_owned()))],
            None,
            &Modes::default(),
        );
        let recorded = builds_finished(
            &[compiled(Built::Recorded("test".to_owned()))],
            None,
            &Modes::default(),
        );
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
            let finished = builds_finished(&builds, None, &Modes::default());
            let runs = audit_runs(&builds, None, &Modes::default());
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

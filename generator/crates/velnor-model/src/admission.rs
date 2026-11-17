// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{GateNeed, PackageName, PlanFrom, RootSlug, When};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Admission {
    pub when: When,
    pub gate: Option<Gate>,
    pub skip_on_draft: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gate {
    Plan(PlanGate),
    Any(Vec<Admission>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanGate {
    pub slug: RootSlug,
    pub need: GateNeed,
    pub from: PlanFrom,
    pub package: Option<PackageName>,
}

impl Gate {
    #[must_use]
    pub fn outputs(&self) -> Vec<(RootSlug, GateNeed)> {
        match self {
            Self::Plan(gate) => gate.outputs(),
            Self::Any(admissions) => admissions
                .iter()
                .flat_map(|admission| admission.gate.iter().flat_map(Self::outputs))
                .collect(),
        }
    }
}

impl PlanGate {
    #[must_use]
    pub fn outputs(&self) -> Vec<(RootSlug, GateNeed)> {
        let own = |need| (self.slug.clone(), need);
        match self.need {
            GateNeed::Doctests | GateNeed::Docs => vec![
                own(GateNeed::Selected),
                own(self.need),
                own(GateNeed::PackageArgs),
            ],
            GateNeed::Policy => vec![own(GateNeed::Deny), own(GateNeed::Machete)],
            _ => vec![own(self.need)],
        }
    }
}
